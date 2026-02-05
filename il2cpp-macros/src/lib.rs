use std::boxed;

use darling::FromMeta;
use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, Ident, ItemFn, ItemStruct, LitStr, Pat, parse_macro_input};

#[derive(Debug, FromMeta)]
#[darling(derive_syn_parse)]
struct MethodMacroArgs {
    name: LitStr,
    #[darling(default)]
    args: darling::util::SpannedValue<Vec<LitStr>>,
}

/// Generates an IL2CPP method wrapper that resolves and caches the target method,
/// then calls it. The generated function returns `Result<_, Il2CppError>`.
#[proc_macro_attribute]
pub fn il2cpp_method(args: TokenStream, input: TokenStream) -> TokenStream {
    // Parse macro arguments
    let macro_args: MethodMacroArgs = match syn::parse(args) {
        Ok(parsed_args) => {
            eprintln!("[il2cpp_method] Successfully parsed macro arguments");
            parsed_args
        }
        Err(parse_error) => {
            eprintln!(
                "[il2cpp_method] ERROR: Failed to parse macro arguments: {}",
                parse_error
            );
            return parse_error.to_compile_error().into();
        }
    };

    // Parse function definition
    let function_def: ItemFn = syn::parse_macro_input!(input as ItemFn);
    let function_name = &function_def.sig.ident;
    eprintln!("[il2cpp_method] Processing function: {}", function_name);

    let function_signature = &function_def.sig;
    let function_return_type = &function_def.sig.output;

    let il2cpp_method_name = &macro_args.name;
    let il2cpp_method_args: Vec<_> = macro_args.args.iter().collect();
    let il2cpp_method_arg_count = il2cpp_method_args.len();

    eprintln!(
        "[il2cpp_method] IL2CPP method: {}",
        il2cpp_method_name.value()
    );
    eprintln!(
        "[il2cpp_method] IL2CPP argument count: {}",
        il2cpp_method_arg_count
    );

    // Check if function has `self` parameter (instance method vs static method)
    let is_static_method = !function_def
        .sig
        .inputs
        .iter()
        .any(|arg: &FnArg| matches!(arg, FnArg::Receiver(_)));
    eprintln!("[il2cpp_method] Is static method: {}", is_static_method);

    // Extract parameter information (skip `self`)
    let (parameter_names, parameter_types) = extract_function_parameters(&function_def);

    if parameter_names.len() != il2cpp_method_arg_count {
        let error_message = format!(
            "[il2cpp_method] ERROR: Parameter mismatch for '{}' - Rust function has {} parameters but IL2CPP args has {} arguments",
            function_name,
            parameter_names.len(),
            il2cpp_method_arg_count
        );
        eprintln!("{}", error_message);

        let compile_error =
            syn::Error::new_spanned(function_signature, error_message).to_compile_error();

        return TokenStream::from(compile_error);
    }

    eprintln!(
        "[il2cpp_method] Extracted {} parameters - VALIDATION PASSED",
        parameter_names.len()
    );

    // Generate method call based on whether it's static or instance method
    let method_call = if is_static_method {
        quote! { cached_method(#(#parameter_names),*) }
    } else {
        quote! { cached_method(self.0, #(#parameter_names),*) }
    };

    let extern_fn_params = if is_static_method {
        quote! { #(#parameter_types),* }
    } else {
        quote! { *const std::ffi::c_void, #(#parameter_types),* }
    };

    let class_retrieval = if is_static_method {
        quote! {
            let class = Self::get_class_static().expect("Failed to get IL2CPP class for static method");
        }
    } else {
        quote! {
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
        #function_signature {
            #class_retrieval

            static IL2CPP_METHOD_CACHE: std::sync::OnceLock<
                Result<extern "C" fn(#extern_fn_params) -> #il2cpp_return_type, Il2CppError>
            > = std::sync::OnceLock::new();

            let cached_method = match IL2CPP_METHOD_CACHE.get_or_init(|| {
                #[cfg(feature = "log")]
                log::debug!(
                    "[il2cpp_method] Resolving IL2CPP method '{}' with {} arguments",
                    stringify!(#il2cpp_method_name),
                    #il2cpp_method_arg_count
                );

                let method_info = class
                    .find_method(#il2cpp_method_name, vec![#(#il2cpp_method_args),*])
                    .expect("Failed to find IL2CPP method");

                #[cfg(feature = "log")]
                log::debug!("[il2cpp_method] Successfully resolved method at address: {:p}", method_info.va());

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

    for (index, arg) in function_def.sig.inputs.iter().enumerate() {
        match arg {
            FnArg::Receiver(_) => {
                eprintln!(
                    "[extract_function_parameters] Skipping 'self' parameter at index {}",
                    index
                );
            }
            FnArg::Typed(pat_type) => {
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    let param_name = pat_ident.ident.clone();
                    let param_type = *pat_type.ty.clone();

                    eprintln!(
                        "[extract_function_parameters] Found parameter: {} at index {}",
                        param_name, index
                    );

                    parameter_names.push(param_name);
                    parameter_types.push(param_type);
                } else {
                    eprintln!(
                        "[extract_function_parameters] WARNING: Could not extract identifier from pattern at index {}",
                        index
                    );
                }
            }
        }
    }

    eprintln!(
        "[extract_function_parameters] Extracted {} total parameters",
        parameter_names.len()
    );
    (parameter_names, parameter_types)
}

/// Generates a reference type wrapper for an IL2CPP class (managed reference type).
#[proc_macro_attribute]
pub fn il2cpp_ffi_ref_type(attr: TokenStream, item: TokenStream) -> TokenStream {
    eprintln!("[il2cpp_ffi_type] Starting FFI type generation");

    let arg = parse_macro_input!(attr as syn::LitStr);
    let fully_qualified_name = arg.value();
    eprintln!("[il2cpp_ffi_type] Type name: {}", fully_qualified_name);

    let ItemStruct { ident, .. } = parse_macro_input!(item as ItemStruct);
    eprintln!("[il2cpp_ffi_type] Struct identifier: {}", ident);

    let ffi_type_expanded = generate_ffi_type_struct(&ident);
    let expanded = quote! {
        #ffi_type_expanded

        impl Il2CppObject for #ident {
            fn ffi_name() -> &'static str {
                #arg
            }
        }
    };

    eprintln!(
        "[il2cpp_ffi_type] Successfully generated FFI type for {}",
        ident
    );
    TokenStream::from(expanded)
}

/// Generates a value type wrapper plus a `__Boxed` ref wrapper for IL2CPP value types.
/// The original struct is emitted as `#[repr(C)]` and `Copy`.
#[proc_macro_attribute]
pub fn il2cpp_ffi_value_type(attr: TokenStream, item: TokenStream) -> TokenStream {
    eprintln!("[il2cpp_ffi_type] Starting FFI type generation");

    let arg = parse_macro_input!(attr as LitStr);
    let input = parse_macro_input!(item as ItemStruct);
    let ident = &input.ident;
    let boxed_ident = Ident::new(&format!("{}__Boxed", ident), ident.span());

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

        impl Il2CppObject for #ident {
            fn ffi_name() -> &'static str {
                #arg
            }
        }

        impl Il2CppObject for #boxed_ident {
            fn ffi_name() -> &'static str {
                #arg
            }
        }
    };

    eprintln!(
        "[il2cpp_ffi_type] Successfully generated FFI type for {}",
        ident
    );
    TokenStream::from(expanded)
}

/// Generates a raw FFI pointer wrapper type for IL2CPP internal structs.
#[proc_macro_attribute]
pub fn ffi_type(_attr: TokenStream, item: TokenStream) -> TokenStream {
    eprintln!("[ffi_type] Starting basic FFI type generation");

    let ItemStruct { ident, .. } = parse_macro_input!(item as ItemStruct);
    eprintln!("[ffi_type] Struct identifier: {}", ident);

    let generated = generate_ffi_type_struct(&ident);
    eprintln!("[ffi_type] Successfully generated FFI type for {}", ident);

    TokenStream::from(generated)
}

/// Generate the FFI transparent struct wrapper
fn generate_ffi_type_struct(struct_ident: &syn::Ident) -> proc_macro2::TokenStream {
    eprintln!(
        "[generate_ffi_type_struct] Generating struct for: {}",
        struct_ident
    );
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
        Ok(parsed_args) => {
            eprintln!("[il2cpp_field] Successfully parsed field arguments");
            parsed_args
        }
        Err(parse_error) => {
            eprintln!(
                "[il2cpp_field] ERROR: Failed to parse field arguments: {}",
                parse_error
            );
            return parse_error.to_compile_error().into();
        }
    };

    let function_def: ItemFn = syn::parse_macro_input!(input as ItemFn);
    let rust_field_name = &function_def.sig.ident;
    let il2cpp_field_name = &field_args.name;
    
    // Extract the actual return type from `-> Type`
    let field_return_type = match &function_def.sig.output {
        syn::ReturnType::Default => {
            eprintln!("[il2cpp_field] ERROR: Field function must have explicit return type");
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
        eprintln!("{}", error_message);

        let compile_error =
            syn::Error::new_spanned(&function_def.sig.inputs, error_message).to_compile_error();

        return TokenStream::from(compile_error);
    }

    eprintln!("[il2cpp_field] Processing field: {}", rust_field_name);
    eprintln!(
        "[il2cpp_field] IL2CPP field name: {}",
        il2cpp_field_name.value()
    );
    eprintln!("[il2cpp_field] Has self (instance field): {}", has_self);
    eprintln!("[il2cpp_field] Parameter validation PASSED");

    let expanded = if has_self {
        // Instance field getter
        quote! {
            pub fn #rust_field_name(&self) -> Result<#field_return_type, Il2CppError> {
                let class = crate::types::System_RuntimeType::from_class(self.get_class())?;
                let field_info = class.get_field(#il2cpp_field_name)?;

                #[cfg(feature = "log")]
                log::debug!("[il2cpp_field] Resolving instance field '{}'", #il2cpp_field_name);

                let value = field_info.get_value(self.0)?;
                Ok(unsafe { std::mem::transmute(value) })
            }
        }
    } else {
        // Static field getter
        quote! {
            pub fn #rust_field_name() -> Result<#field_return_type, Il2CppError> {
                let class = crate::types::System_RuntimeType::from_class(Self::get_class_static()?)?;
                let field_info = class.get_field(#il2cpp_field_name)?;

                #[cfg(feature = "log")]
                log::debug!("[il2cpp_field] Resolving static field '{}'", #il2cpp_field_name);

                let value = field_info.get_value(core::ptr::null())?;
                Ok(unsafe { std::mem::transmute(value) })
            }
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