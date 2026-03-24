pub mod api;
pub mod errors;
pub mod types;
pub mod utils;

extern crate self as il2cpp_runtime;

pub use il2cpp_macros::{
    ffi_type, il2cpp_enum_type, il2cpp_field, il2cpp_getter_property, il2cpp_method,
    il2cpp_ref_type, il2cpp_value_type,
};
pub use types::{
    Il2CppClass, Il2CppDomain, Il2CppField, Il2CppMethod, Il2CppObject, Il2CppRefType,
    Il2CppType, System_RuntimeType,
};

pub mod prelude {
    pub use crate::errors::Il2CppError;
    pub use crate::types::*;
    pub use crate::{
        ffi_type, il2cpp_enum_type, il2cpp_field, il2cpp_getter_property, il2cpp_method,
        il2cpp_ref_type, il2cpp_value_type,
    };
}

pub fn __log_debug(args: std::fmt::Arguments<'_>) {
    #[cfg(feature = "log")]
    log::debug!("{}", args);
}

use crate::errors::Il2CppError;
use std::{borrow::Cow, collections::HashMap, sync::OnceLock};

static API_TABLE_OFFSET: OnceLock<usize> = OnceLock::new();

static TYPE_TABLE: OnceLock<HashMap<Cow<'static, str>, Il2CppClass>> = OnceLock::new();

pub fn get_cached_class<S: AsRef<str>>(key: S) -> Result<Il2CppClass, Il2CppError> {
    TYPE_TABLE
        .get()
        .ok_or(Il2CppError::TypeTableError)?
        .get(key.as_ref())
        .ok_or_else(|| Il2CppError::CachedClassError(key.as_ref().to_string()))
        .cloned()
}

pub fn get_type_table() -> Result<&'static HashMap<Cow<'static, str>, Il2CppClass>, Il2CppError> {
    TYPE_TABLE.get().ok_or(Il2CppError::TypeTableError)
}

pub fn init(api_table_offset: usize, indexes: api::ApiIndexTable) -> Result<(), Il2CppError> {
    let _ = API_TABLE_OFFSET.set(api_table_offset);
    api::set_api_indexes(indexes);
    let mut type_table = HashMap::new();

    let domain = api::il2cpp_domain_get();
    api::il2cpp_thread_attach(domain);

    for assembly in domain.assemblies() {
        let image = api::il2cpp_assembly_get_image(assembly);

        for class in image.classes() {
            let type_name = class.byval_arg().name();
            type_table.insert(type_name, class);
        }
    }
    TYPE_TABLE.set(type_table).unwrap();
    Ok(())
}
