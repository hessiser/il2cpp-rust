use std::boxed;

use darling::FromMeta;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Expr, FnArg, Ident, ItemEnum, ItemFn, ItemStruct, LitStr, Pat, parse_macro_input};

#[derive(Debug, FromMeta)]
#[darling(derive_syn_parse)]
struct MethodMacroArgs {
    name: LitStr,
    #[darling(default)]
    args: darling::util::SpannedValue<Vec<LitStr>>,
    #[darling(default)]
    extension: bool,
}

/// Generates an IL2CPP method wrapper that resolves and caches the target method,
/// then calls it. The generated function returns `Result<_, Il2CppError>`.
#[proc_macro_attribute]
pub fn il2cpp_method(args: TokenStream, input: TokenStream) -> TokenStream {
    // Parse macro arguments
    let macro_args: MethodMacroArgs = match syn::parse(args) {
        Ok(parsed_args) => parsed_args,
        Err(parse_error) => {
            return parse_error.to_compile_error().into();
        }
    };

    // Parse function definition
    let function_def: ItemFn = syn::parse_macro_input!(input as ItemFn);
    let function_name = &function_def.sig.ident;

    let function_signature = &function_def.sig;
    let function_vis = &function_def.vis;
    let _function_return_type = &function_def.sig.output;

    let il2cpp_method_name = &macro_args.name;
    let il2cpp_method_args: Vec<_> = macro_args.args.iter().collect();
    let il2cpp_method_arg_count = il2cpp_method_args.len();

    // Check if function has `self` parameter (instance method vs static method)
    let is_static_method = !function_def
        .sig
        .inputs
        .iter()
        .any(|arg: &FnArg| matches!(arg, FnArg::Receiver(_)));
    // Extract parameter information (skip `self`)
    let (parameter_names, parameter_types) = extract_function_parameters(&function_def);

    if parameter_names.len() != il2cpp_method_arg_count {
        let error_message = format!(
            "[il2cpp_method] ERROR: Parameter mismatch for '{}' - Rust function has {} parameters but IL2CPP args has {} arguments",
            function_name,
            parameter_names.len(),
            il2cpp_method_arg_count
        );
        let compile_error =
            syn::Error::new_spanned(function_signature, error_message).to_compile_error();

        return TokenStream::from(compile_error);
    }

    // Generate method call based on whether it's static, instance, or extension method
    let method_call = if is_static_method {
        quote! { cached_method(#(#parameter_names),*) }
    } else if macro_args.extension {
        quote! { cached_method(core::ptr::null(), #(#parameter_names),*) }
    } else {
        quote! { cached_method(self.0, #(#parameter_names),*) }
    };

    let extern_fn_params = if is_static_method {
        quote! { #(#parameter_types),* }
    } else {
        quote! { *const std::ffi::c_void, #(#parameter_types),* }
    };

    let class_retrieval = if is_static_method || macro_args.extension {
        quote! {
            let class = match Self::get_class_static() {
                Ok(class) => class,
                Err(e) => {
                    return Err(e);
                }
            };
        }
    } else {
        quote! {
            if self.0.is_null() {
                return Err(::il2cpp_runtime::errors::Il2CppError::NullPointerDereference);
            }
            let class = self.get_class();
        }
    };

    let il2cpp_return_type: syn::Type = match &function_def.sig.output {
        syn::ReturnType::Default => syn::parse_quote!(()),
        syn::ReturnType::Type(_, ty) => (**ty).clone(),
    };

    let mut function_signature = function_def.sig.clone();
    function_signature.output = syn::parse_quote!(-> Result<#il2cpp_return_type, Il2CppError>);

    let expanded = quote! {
        #function_vis #function_signature {
            static IL2CPP_METHOD_CACHE: std::sync::OnceLock<
                Result<extern "C" fn(#extern_fn_params) -> #il2cpp_return_type, Il2CppError>
            > = std::sync::OnceLock::new();

            let cached_method = match IL2CPP_METHOD_CACHE.get_or_init(|| {
                #class_retrieval

                ::il2cpp_runtime::__log_debug(format_args!(
                    "[il2cpp_method] Resolving {}::{} (args: {}, static: {})",
                    class.name(),
                    stringify!(#il2cpp_method_name),
                    stringify!(#(#il2cpp_method_args),*),
                    #is_static_method
                ));

                let method_info = match class
                    .find_method(#il2cpp_method_name, vec![#(#il2cpp_method_args),*])
                {
                    Ok(method_info) => {
                        ::il2cpp_runtime::__log_debug(format_args!(
                            "[il2cpp_method] Resolved {}::{}",
                            class.name(),
                            stringify!(#il2cpp_method_name)
                        ));
                        method_info
                    }
                    Err(e) => {
                        ::il2cpp_runtime::__log_debug(format_args!(
                            "[il2cpp_method] Failed to resolve {}::{}: {}",
                            class.name(),
                            stringify!(#il2cpp_method_name),
                            e
                        ));
                        return Err(e);
                    }
                };

                Ok(unsafe { std::mem::transmute(method_info.va()) })
            }) {
                Ok(f) => *f,
                Err(e) => return Err(e.clone()),
            };
            Ok(#method_call)
        }
    };

    TokenStream::from(expanded)
}


/// Extract parameter names and types from function signature, skipping `self`
fn extract_function_parameters(function_def: &ItemFn) -> (Vec<syn::Ident>, Vec<syn::Type>) {
    let mut parameter_names: Vec<syn::Ident> = Vec::new();
    let mut parameter_types: Vec<syn::Type> = Vec::new();

    for arg in function_def.sig.inputs.iter() {
        match arg {
            FnArg::Receiver(_) => {}
            FnArg::Typed(pat_type) => {
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    let param_name = pat_ident.ident.clone();
                    let param_type = *pat_type.ty.clone();

                    parameter_names.push(param_name);
                    parameter_types.push(param_type);
                }
            }
        }
    }
    (parameter_names, parameter_types)
}

fn impl_il2cpp_object(
    ident: &Ident,
    ffi_name: &LitStr,
    as_ptr_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        impl Il2CppObject for #ident {
            fn ffi_name() -> &'static str {
                #ffi_name
            }
            fn as_ptr(&self) -> *const std::ffi::c_void {
                #as_ptr_expr
            }
        }
    }
}

fn impl_il2cpp_ref_type(
    ident: &Ident,
    ffi_name: &LitStr,
    as_ptr_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let object_impl = impl_il2cpp_object(ident, ffi_name, as_ptr_expr);
    quote! {
        impl Il2CppRefType for #ident {}
        #object_impl
    }
}

/// Generates a reference type wrapper for an IL2CPP class (managed reference type).
#[proc_macro_attribute]
pub fn il2cpp_ref_type(attr: TokenStream, item: TokenStream) -> TokenStream {
    let arg = parse_macro_input!(attr as syn::LitStr);

    let ItemStruct { ident, .. } = parse_macro_input!(item as ItemStruct);

    let ffi_type_expanded = generate_ffi_type_struct(&ident);
    let ref_impls = impl_il2cpp_ref_type(&ident, &arg, quote! { self.0 });
    let expanded = quote! {
        #ffi_type_expanded
        #ref_impls
    };

    TokenStream::from(expanded)
}

/// Generates a value type wrapper plus a `__Boxed` ref wrapper for IL2CPP value types.
/// The original struct is emitted as `#[repr(C)]` and `Copy`.
#[proc_macro_attribute]
pub fn il2cpp_value_type(attr: TokenStream, item: TokenStream) -> TokenStream {
    let arg = parse_macro_input!(attr as LitStr);
    let input = parse_macro_input!(item as ItemStruct);
    let ident = &input.ident;
    let boxed_ident = Ident::new(&format!("{}__Boxed", ident), ident.span());

    let value_object_impl = impl_il2cpp_object(
        ident,
        &arg,
        quote! { self as *const _ as *const std::ffi::c_void },
    );
    let boxed_ref_impls = impl_il2cpp_ref_type(&boxed_ident, &arg, quote! { self.0 });

    let expanded = quote! {
        #[repr(C)]
        #[derive(Debug, Copy, Clone)]
        #input

        #[repr(transparent)]
        #[derive(Debug, Copy, Clone, Eq, PartialEq)]
        pub struct #boxed_ident(pub *const std::ffi::c_void);

        unsafe impl Send for #boxed_ident {}
        unsafe impl Sync for #boxed_ident {}

        impl #boxed_ident {
            pub unsafe fn unbox(&self) -> Result<#ident, Il2CppError> {
                if self.0.is_null() {
                    Err(Il2CppError::NullPointerDereference)
                } else {
                    Ok(unsafe { *(self.0 as *const #ident).byte_offset(0x10) })
                }
            }
        }
        #value_object_impl

        impl Il2CppValueType for #ident {}

        #boxed_ref_impls
    };

    TokenStream::from(expanded)
}

/// Generates a raw FFI pointer wrapper type for IL2CPP internal structs.
#[proc_macro_attribute]
pub fn ffi_type(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let ItemStruct { ident, .. } = parse_macro_input!(item as ItemStruct);
    let generated = generate_ffi_type_struct(&ident);

    TokenStream::from(generated)
}

/// Generate the FFI transparent struct wrapper
fn generate_ffi_type_struct(struct_ident: &syn::Ident) -> proc_macro2::TokenStream {
    quote! {
        #[repr(transparent)]
        #[derive(Debug, Copy, Clone, Eq, PartialEq)]
        pub struct #struct_ident(pub *const std::ffi::c_void);
        

        unsafe impl Send for #struct_ident {}
        unsafe impl Sync for #struct_ident {}
    }
}


#[derive(Debug, FromMeta)]
#[darling(derive_syn_parse)]
struct FieldMacroArgs {
    name: LitStr,
}

/// Generates a field getter that returns `Result<T, Il2CppError>`.
/// **Do not return ValueType structs** from `#[il2cpp_field]`; return the boxed/ref
/// wrapper instead, because fields are accessed via object references.
#[proc_macro_attribute]
pub fn il2cpp_field(args: TokenStream, input: TokenStream) -> TokenStream {
    // Parse macro arguments
    let field_args: FieldMacroArgs = match syn::parse(args) {
        Ok(parsed_args) => parsed_args,
        Err(parse_error) => {
            return parse_error.to_compile_error().into();
        }
    };

    let function_def: ItemFn = syn::parse_macro_input!(input as ItemFn);
    let rust_field_name = &function_def.sig.ident;
    let il2cpp_field_name = &field_args.name;
    
    // Extract the actual return type from `-> Type`
    let field_return_type = match &function_def.sig.output {
        syn::ReturnType::Default => {
            return syn::Error::new_spanned(&function_def.sig, "Field function must have explicit return type").to_compile_error().into();
        }
        syn::ReturnType::Type(_, ty) => ty,
    };

    // Check if function has `self` parameter
    let has_self = function_def
        .sig
        .inputs
        .iter()
        .any(|arg| matches!(arg, FnArg::Receiver(_)));

    // Validate that function has no other parameters (besides self)
    let param_count = function_def.sig.inputs.len();
    let expected_param_count = if has_self { 1 } else { 0 };

    if param_count != expected_param_count {
        let error_message = format!(
            "[il2cpp_field] ERROR: Field function '{}' must have no parameters (only optional self), but found {} parameters",
            rust_field_name,
            if has_self {
                param_count - 1
            } else {
                param_count
            }
        );
        let compile_error =
            syn::Error::new_spanned(&function_def.sig.inputs, error_message).to_compile_error();

        return TokenStream::from(compile_error);
    }

    let receiver = if has_self {
        quote! { &self }
    } else {
        quote! {}
    };

    let null_check = if has_self {
        quote! {
            if self.0.is_null() {
                return Err(::il2cpp_runtime::errors::Il2CppError::NullPointerDereference);
            }
        }
    } else {
        quote! {}
    };

    let class_expr = if has_self {
        quote! { ::il2cpp_runtime::System_RuntimeType::from_class(self.get_class())? }
    } else {
        quote! { ::il2cpp_runtime::System_RuntimeType::from_class(Self::get_class_static()?)? }
    };

    let instance_expr = if has_self {
        quote! { self.0 }
    } else {
        quote! { core::ptr::null() }
    };

    let expanded = quote! {
        pub fn #rust_field_name(#receiver) -> Result<#field_return_type, Il2CppError>
        where 
            #field_return_type: ::il2cpp_runtime::Il2CppRefType,
        {
            #null_check
            let class = #class_expr;
            let field_info = class.get_field(#il2cpp_field_name)?;

            ::il2cpp_runtime::__log_debug(format_args!(
                "[il2cpp_field] Resolving {}::{}",
                class.get_il2cpp_type().name(),
                #il2cpp_field_name
            ));
            let value = field_info.get_value(#instance_expr)?;
            if value.is_null() {
                return Err(::il2cpp_runtime::errors::Il2CppError::NullPointerDereference);
            }
            Ok(unsafe { std::mem::transmute(value) })
        }
    };
    TokenStream::from(expanded)
}

#[derive(Debug, FromMeta)]
#[darling(derive_syn_parse)]
struct GetterPropertyArgs {
    property: LitStr,
}

/// Generates a property getter by expanding to `#[il2cpp_method(name = "get_<Property>", args = [])]`.
#[proc_macro_attribute]
pub fn il2cpp_getter_property(args: TokenStream, input: TokenStream) -> TokenStream {
    let getter_args: GetterPropertyArgs = match syn::parse(args) {
        Ok(parsed_args) => parsed_args,
        Err(parse_error) => return parse_error.to_compile_error().into(),
    };

    let function_def: ItemFn = parse_macro_input!(input as ItemFn);
    let getter_name = format!("get_{}", getter_args.property.value());

    let expanded = quote! {
        #[il2cpp_method(name = #getter_name, args = [])]
        #function_def
    };

    TokenStream::from(expanded)
}

/// Generates a repr'd enum with sequential discriminants starting at 0.
#[proc_macro_attribute]
pub fn il2cpp_enum_type(attr: TokenStream, item: TokenStream) -> TokenStream {
    let repr_type: syn::Type = match syn::parse(attr) {
        Ok(parsed) => parsed,
        Err(parse_error) => return parse_error.to_compile_error().into(),
    };

    let mut enum_def = parse_macro_input!(item as ItemEnum);
    enum_def
        .attrs
        .push(syn::parse_quote!(#[repr(#repr_type)]));

    for (index, variant) in enum_def.variants.iter_mut().enumerate() {
        let value: Expr = syn::parse_quote!(#index);
        variant.discriminant = Some((Default::default(), value));
    }

    TokenStream::from(quote! { #enum_def })
}