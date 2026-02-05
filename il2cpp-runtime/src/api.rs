use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;

use crate::types::{Il2CppAssembly, Il2CppClass, Il2CppDomain, Il2CppField, Il2CppImage, Il2CppMethod, Il2CppType};

pub static API_INDEXES: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();

pub const API_FUNCTION_ORDER: [&str; 16] = [
    "il2cpp_assembly_get_image",
    "il2cpp_class_get_methods",
    "il2cpp_class_get_name",
    "il2cpp_class_from_type",
    "il2cpp_domain_get",
    "il2cpp_domain_get_assemblies",
    "il2cpp_field_get_name",
    "il2cpp_field_get_value_object",
    "il2cpp_method_get_return_type",
    "il2cpp_method_get_name",
    "il2cpp_method_get_param_count",
    "il2cpp_method_get_param",
    "il2cpp_thread_attach",
    "il2cpp_type_get_name",
    "il2cpp_image_get_class_count",
    "il2cpp_image_get_class",
];

pub fn set_api_indexes(indexes: HashMap<&'static str, usize>) {
    let _ = API_INDEXES.set(indexes);
}

pub fn set_api_indexes_ordered(indexes: &[usize]) {
    assert_eq!(
        indexes.len(),
        API_FUNCTION_ORDER.len(),
        "API index count mismatch"
    );

    let map = API_FUNCTION_ORDER
        .iter()
        .copied()
        .zip(indexes.iter().copied())
        .collect::<HashMap<_, _>>();

    let _ = API_INDEXES.set(map);
}

macro_rules! il2cpp_api {
    ($name:ident($($arg_name:ident: $arg_type:ty),*) -> $ret_type:ty) => {
        #[allow(warnings)]
        pub fn $name($($arg_name: $arg_type,)*) -> $ret_type {
            unsafe {
                type FuncType = unsafe extern "fastcall" fn($($arg_type,)*) -> $ret_type;
                let index = *API_INDEXES
                    .get()
                    .and_then(|m| m.get(stringify!($name)))
                    .expect("API index not initialized for this function");

                ::std::mem::transmute::<usize, FuncType>(
                    *((*crate::API_TABLE_OFFSET.get().expect("API_TABLE_OFFSET not initialized") + 8 * index) as *const usize)
                )($($arg_name,)*)
           }
        }
    };
}

il2cpp_api!(il2cpp_assembly_get_image(assembly: Il2CppAssembly) -> Il2CppImage);
il2cpp_api!(il2cpp_class_get_methods(klass: Il2CppClass, iter: *const *const usize) -> Il2CppMethod);
il2cpp_api!(il2cpp_class_get_name(klass: Il2CppClass) -> *const i8);
il2cpp_api!(il2cpp_class_from_type(r#type: Il2CppType) -> Il2CppClass);
il2cpp_api!(il2cpp_domain_get() -> Il2CppDomain);
il2cpp_api!(il2cpp_domain_get_assemblies(domain: Il2CppDomain, size: *mut usize) -> *mut Il2CppAssembly);
il2cpp_api!(il2cpp_field_get_name(field: Il2CppField) -> *const i8);
il2cpp_api!(il2cpp_field_get_value_object(field: Il2CppField, obj: *const c_void) -> *const c_void);
il2cpp_api!(il2cpp_method_get_return_type(method: Il2CppMethod) -> Il2CppType);
il2cpp_api!(il2cpp_method_get_name(method: Il2CppMethod) -> *const i8);
il2cpp_api!(il2cpp_method_get_param_count(method: Il2CppMethod) -> u32);
il2cpp_api!(il2cpp_method_get_param(method: Il2CppMethod, index: u32) -> Il2CppType);
il2cpp_api!(il2cpp_thread_attach(domain: Il2CppDomain) -> usize);
il2cpp_api!(il2cpp_type_get_name(r#type: Il2CppType) -> *const i8);
il2cpp_api!(il2cpp_image_get_class_count(image: Il2CppImage) -> usize);
il2cpp_api!(il2cpp_image_get_class(image: Il2CppImage, index: usize) -> Il2CppClass);