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

pub fn __log_debug(_args: std::fmt::Arguments<'_>) {
    #[cfg(feature = "log")]
    log::debug!("{}", _args);
}

pub fn __log_error(_args: std::fmt::Arguments<'_>) {
    #[cfg(feature = "log")]
    log::error!("{}", _args);
}

use crate::errors::Il2CppError;
use std::{
    any::Any,
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::OnceLock,
};

static API_TABLE_OFFSET: OnceLock<usize> = OnceLock::new();

static TYPE_TABLE: OnceLock<HashMap<String, Il2CppClass>> = OnceLock::new();

fn try_read_class_identity(class: Il2CppClass) -> Option<(String, String)> {
    match microseh::try_seh(|| catch_unwind(AssertUnwindSafe(|| (class.namespace(), class.name())))) {
        Ok(Ok((namespace, name))) => Some((namespace, name)),
        Ok(Err(_)) | Err(_) => None,
    }
}

fn find_class_by_qualified_identity(
    table: &HashMap<String, Il2CppClass>,
    key: &str,
) -> Option<Il2CppClass> {
    let (expected_namespace, expected_name) = key.rsplit_once('.')?;
    let mut matches = table.values().copied().filter(|class| {
        try_read_class_identity(*class)
            .map(|(namespace, name)| namespace == expected_namespace && name == expected_name)
            .unwrap_or(false)
    });

    let class = matches.next()?;
    if matches.next().is_none() {
        Some(class)
    } else {
        None
    }
}

pub fn get_cached_class<S: AsRef<str>>(key: S) -> Result<Il2CppClass, Il2CppError> {
    let key = key.as_ref();
    let table = TYPE_TABLE.get().ok_or(Il2CppError::TypeTableError)?;

    if let Some(class) = table.get(key) {
        if !key.contains('.') {
            return Ok(*class);
        }

        if try_read_class_identity(*class)
            .map(|(namespace, name)| format!("{namespace}.{name}") == key)
            .unwrap_or(false)
        {
            return Ok(*class);
        }
    }

    if key.contains('.') {
        if let Some(class) = find_class_by_qualified_identity(table, key) {
            return Ok(class);
        }
    }

    let short_key = key.rsplit('.').next().unwrap_or(key);
    let mut matches = table
        .iter()
        .filter(|(qualified_name, _)| qualified_name.rsplit('.').next() == Some(short_key));

    if let Some((_, class)) = matches.next() {
        if matches.next().is_none() {
            return Ok(*class);
        }
    }

    Err(Il2CppError::CachedClassError(key.to_string()))
}

pub fn get_type_table() -> Result<&'static HashMap<String, Il2CppClass>, Il2CppError> {
    TYPE_TABLE.get().ok_or(Il2CppError::TypeTableError)
}

fn panic_payload_to_string(panic: Box<dyn Any + Send>) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Unknown panic payload".to_string())
}

fn protected_stage<T, F>(stage: &str, action: F) -> Result<T, Il2CppError>
where
    F: FnOnce() -> T,
{
    let mut action = Some(action);
    match microseh::try_seh(|| {
        catch_unwind(AssertUnwindSafe(|| {
            action.take().expect("protected_stage called more than once")()
        }))
    }) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(panic)) => Err(Il2CppError::RuntimeStageFailure(format!(
            "panic during {stage}: {}",
            panic_payload_to_string(panic)
        ))),
        Err(seh) => Err(Il2CppError::RuntimeStageFailure(format!(
            "SEH during {stage}: {seh:?}"
        ))),
    }
}

pub fn init(api_table_offset: usize, indexes: api::ApiIndexTable) -> Result<(), Il2CppError> {
    let _ = API_TABLE_OFFSET.set(api_table_offset);
    api::set_api_indexes(indexes);
    let mut type_table = HashMap::new();

    let domain = protected_stage("il2cpp_domain_get", api::il2cpp_domain_get)?;

    let _ = protected_stage("il2cpp_thread_attach", || api::il2cpp_thread_attach(domain));

    let assemblies = protected_stage("il2cpp_domain_get_assemblies", || domain.assemblies())?;

    for (assembly_index, assembly) in assemblies.into_iter().enumerate() {
        let image = match protected_stage(
            &format!("il2cpp_assembly_get_image[{assembly_index}]"),
            || api::il2cpp_assembly_get_image(assembly),
        ) {
            Ok(image) => image,
            Err(_) => {
                continue;
            }
        };

        let class_count = match protected_stage(
            &format!("il2cpp_image_get_class_count[{assembly_index}]"),
            || image.class_count(),
        ) {
            Ok(class_count) => class_count,
            Err(_) => {
                continue;
            }
        };

        for class_index in 0..class_count {
            let raw_handle = match protected_stage(
                &format!("il2cpp_image_get_class[{assembly_index}:{class_index}]"),
                || api::il2cpp_image_get_class(image, class_index).0 as usize,
            ) {
                Ok(raw_handle) => raw_handle,
                Err(_) => {
                    continue;
                }
            };

            let class = match protected_stage(
                &format!("Il2CppClass direct handle[{assembly_index}:{class_index}]"),
                || types::Il2CppClass(raw_handle as *const _),
            ) {
                Ok(class) => class,
                Err(_) => {
                    continue;
                }
            };

            let type_name = match protected_stage(
                &format!("Il2CppClass::byval_arg().name()[{assembly_index}:{class_index}]"),
                || class.byval_arg().name(),
            ) {
                Ok(type_name) => type_name,
                Err(_) => {
                    continue;
                }
            };

            let namespace = match protected_stage(
                &format!("il2cpp_class_get_namespace[{assembly_index}:{class_index}]"),
                || unsafe { utils::cstr_to_str(api::il2cpp_class_get_namespace(class)).into_owned() },
            ) {
                Ok(namespace) => namespace,
                Err(_) => {
                    continue;
                }
            };

            let qualified_name = if namespace.is_empty() {
                type_name.clone()
            } else {
                format!("{namespace}.{type_name}")
            };

            type_table.insert(qualified_name, class);
        }
    }

    if type_table.is_empty() {
        return Err(Il2CppError::RuntimeStageFailure(
            "type table remained empty after metadata scan".to_string(),
        ));
    }

    TYPE_TABLE.set(type_table).unwrap();
    Ok(())
}
