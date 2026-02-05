pub mod api;
pub mod errors;
pub mod types;
mod utils;

use crate::{
    errors::Il2CppError,
    types::{Il2CppClass, Il2CppMethod},
};
use std::{borrow::Cow, collections::HashMap, sync::OnceLock};

pub static API_TABLE_OFFSET: OnceLock<usize> = OnceLock::new();

static FUNCTIONS_TABLE: OnceLock<HashMap<String, Il2CppMethod>> = OnceLock::new();
static TYPE_TABLE: OnceLock<HashMap<Cow<'static, str>, Il2CppClass>> = OnceLock::new();

pub fn get_native_method(key: &str) -> Result<Il2CppMethod, Il2CppError> {
    FUNCTIONS_TABLE
        .get()
        .ok_or(Il2CppError::FunctionTableError)?
        .get(key)
        .ok_or_else(|| Il2CppError::NativeMethodError(key.to_string()))
        .cloned()
}

pub fn get_cached_class(key: &str) -> Result<Il2CppClass, Il2CppError> {
    TYPE_TABLE
        .get()
        .ok_or(Il2CppError::TypeTableError)?
        .get(key)
        .ok_or_else(|| Il2CppError::CachedClassError(key.to_string()))
        .cloned()
}

pub fn init() -> Result<(), Il2CppError> {
    let mut method_maps = HashMap::with_capacity(470_000);
    let mut type_table = HashMap::with_capacity(50_000);

    let domain = api::il2cpp_domain_get();
    api::il2cpp_thread_attach(domain);

    for assembly in domain.assemblies() {
        let image = api::il2cpp_assembly_get_image(assembly);

        for class in image.classes() {
            let type_name = class.byval_arg().name();
            for method in class.methods() {
                method_maps.insert(format!("{type_name}::{}", method.format_params()), method);
            }
            type_table.insert(type_name, class);
        }
    }

    FUNCTIONS_TABLE.set(method_maps).unwrap();
    TYPE_TABLE.set(type_table).unwrap();
    Ok(())
}
