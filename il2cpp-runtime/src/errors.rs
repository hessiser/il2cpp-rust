#[derive(thiserror::Error, Debug, Clone)]
pub enum Il2CppError {
    #[error("Null pointer dereference")]
    NullPointerDereference,
    #[error("Method {method} contains {actual} args and not {expected} args")]
    ArgCountMismatch {
        method: String,
        actual: usize,
        expected: usize,
    },
    #[error("Method {method} arg {index} should be {expected}, got {actual}")]
    ArgTypeMismatch {
        method: String,
        index: usize,
        expected: String,
        actual: String,
    },
    #[error("Method {method} returns {actual} and not {expected}")]
    ReturnTypeMismatch {
        method: String,
        actual: String,
        expected: String,
    },
    #[error("No method named {0}")]
    MethodNotFound(String),
    #[error("No method name matched {0}")]
    MethodNameNotMatched(String),
    #[error("No overload of {0} matched signature")]
    NoOverloadMatched(String),
    #[error("Failed to get type table")]
    TypeTableError,
    #[error("Failed to get cached class {0}")]
    CachedClassError(String),
    #[error("Field '{field_name}' not found in type '{type_name}'")]
    FieldNotFound {
        field_name: String,
        type_name: String,
    },
    #[error("Failed to build UTF-16 IL2CPP string from input: {0}")]
    StringConversionError(String),
}

