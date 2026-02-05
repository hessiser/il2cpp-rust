use std::borrow::Cow;
use std::fmt::Display;
use std::os::raw::c_void;
use std::ptr::null;

use il2cpp_macros::{
    ffi_type, il2cpp_ffi_ref_type, il2cpp_ffi_value_type, il2cpp_getter_property, il2cpp_method,
};

use crate::api::{
    il2cpp_class_from_type, il2cpp_class_get_methods, il2cpp_class_get_name,
    il2cpp_domain_get_assemblies, il2cpp_field_get_name, il2cpp_field_get_value_object,
    il2cpp_image_get_class, il2cpp_image_get_class_count, il2cpp_method_get_name,
    il2cpp_method_get_param, il2cpp_method_get_param_count, il2cpp_type_get_name,
};
use crate::errors::Il2CppError;
use crate::{get_cached_class, utils};

#[ffi_type]
pub struct Il2CppClass;

#[ffi_type]
pub struct Il2CppAssembly;

#[ffi_type]
pub struct Il2CppImage;

impl Il2CppImage {
    pub fn class_count(&self) -> usize {
        il2cpp_image_get_class_count(*self)
    }

    pub fn classes(&self) -> Vec<Il2CppClass> {
        (0..self.class_count())
            .map(|index| il2cpp_image_get_class(*self, index))
            .collect()
    }
}

#[ffi_type]
pub struct Il2CppMethod;

impl Il2CppMethod {
    pub fn name(&self) -> Cow<'static, str> {
        unsafe { utils::cstr_to_str(il2cpp_method_get_name(*self)) }
    }

    pub fn class(&self) -> Il2CppClass {
        unsafe { *((self.0) as *const Il2CppClass) }
    }

    pub fn va(&self) -> *const c_void {
        unsafe { *((self.0.byte_offset(8)) as *const *const c_void) }
    }

    pub fn args_cnt(&self) -> u32 {
        il2cpp_method_get_param_count(*self)
    }

    pub fn arg(&self, i: u32) -> Il2CppType {
        il2cpp_method_get_param(*self, i)
    }

    pub fn arg_type_formatted(&self, i: u32) -> String {
        self.arg(i).alias_name()
    }

    pub fn format_params(&self) -> String {
        use std::fmt::Write;
        let param_count = il2cpp_method_get_param_count(*self);
        let name = self.name();
        let mut out = String::with_capacity(0);

        let _ = write!(out, "{name}(");
        for param_index in 0..param_count {
            let param = il2cpp_method_get_param(*self, param_index);
            let _ = write!(out, "{}", param.class().byval_arg().alias_name());

            if param_index + 1 < param_count {
                let _ = write!(out, ",");
            }
        }
        let _ = write!(out, ")");

        out
    }
}

#[ffi_type]
pub struct Il2CppType;

impl Il2CppType {
    pub fn name(&self) -> Cow<'static, str> {
        unsafe { utils::cstr_to_str(il2cpp_type_get_name(*self)) }
    }

    pub fn alias_name(&self) -> String {
        let name = self.name();

        (match name.as_ref() {
            "System.Int32" => "int",
            "System.UInt32" => "uint",
            "System.Int16" => "short",
            "System.UInt16" => "ushort",
            "System.Int64" => "long",
            "System.UInt64" => "ulong",
            "System.Byte" => "byte",
            "System.SByte" => "sbyte",
            "System.Boolean" => "bool",
            "System.Single" => "float",
            "System.Double" => "double",
            "System.String" => "string",
            "System.Char" => "char",
            "System.Object" => "object",
            "System.Void" => "void",
            "System.Decimal" => "decimal",
            "System.DateTime" => "DateTime",
            other => other,
        })
        .to_string()
    }

    pub fn class(&self) -> Il2CppClass {
        il2cpp_class_from_type(*self)
    }
}

#[ffi_type]
pub struct Il2CppField;

impl Il2CppField {
    pub fn name(&self) -> Cow<'static, str> {
        unsafe { utils::cstr_to_str(il2cpp_field_get_name(*self)) }
    }

    pub fn get_value(
        &self,
        instance: *const std::ffi::c_void,
    ) -> Result<*const std::ffi::c_void, Il2CppError> {
        let value = il2cpp_field_get_value_object(*self, instance);

        if value.is_null() {
            Err(Il2CppError::NullPointerDereference)
        } else {
            Ok(value)
        }
    }
}

#[ffi_type]
pub struct Il2CppDomain;

impl Il2CppDomain {
    pub fn assemblies(&self) -> Vec<Il2CppAssembly> {
        let mut count = 0;
        let assemblies = il2cpp_domain_get_assemblies(*self, &mut count);
        unsafe { std::slice::from_raw_parts(assemblies, count).to_vec() }
    }
}

impl Il2CppClass {
    pub fn name(&self) -> Cow<'static, str> {
        unsafe { utils::cstr_to_str(il2cpp_class_get_name(*self)) }
    }

    pub fn byval_arg(&self) -> Il2CppType {
        Il2CppType(unsafe { self.0.byte_offset(128) })
    }

    pub fn methods(&self) -> Vec<Il2CppMethod> {
        let iter = std::ptr::null();
        let mut result = Vec::new();
        loop {
            let method = il2cpp_class_get_methods(*self, &iter);
            if method.0.is_null() {
                break;
            }
            result.push(method)
        }
        result
    }

    // pub fn find_method_by_name(&self, name: &str) -> Option<Il2CppMethod> {
    //     self.methods()
    //         .into_iter()
    //         .find(|&method| method.name() == name)
    // }

    pub fn find_method(
        &self,
        name: &str,
        arg_types: Vec<&str>,
    ) -> Result<Il2CppMethod, Il2CppError> {
        let qualified_name = format!("{}::{}", self.name(), name);

        for method in self.methods().iter().filter(|m| m.name() == name) {
            let count = method.args_cnt() as usize;

            if count != arg_types.len() {
                return Err(Il2CppError::ArgCountMismatch {
                    method: qualified_name.clone(),
                    actual: count,
                    expected: arg_types.len(),
                });
            }

            let mut mismatch: Option<(usize, String)> = None;
            for (i, arg_type) in arg_types.iter().enumerate() {
                if *arg_type != method.arg_type_formatted(i as u32) {
                    mismatch = Some((i, method.arg_type_formatted(i as u32)));
                    break;
                }
            }

            if let Some((i, actual)) = mismatch {
                return Err(Il2CppError::ArgTypeMismatch {
                    method: qualified_name.clone(),
                    index: i,
                    expected: arg_types[i].to_string(),
                    actual,
                });
            }

            return Ok(*method);
        }

        Err(Il2CppError::MethodNotFound(qualified_name))
    }
}

#[il2cpp_ffi_ref_type("System.String")]
pub struct Il2CppString;

impl Il2CppString {
    // Do not use il2cpp_field attribute
    // It is reliant on Il2CppString
    fn len(&self) -> u32 {
        unsafe { *(self.0.byte_offset(24) as *const u32) }
    }

    fn first_char(&self) -> u16 {
        unsafe { *(self.0.byte_offset(32) as *const u16) }
    }

    #[il2cpp_method(name = "CreateString", args = ["char*"])]
    fn create_string(&self, buffer: *const u16) -> Il2CppString {}

    pub fn new<S: AsRef<str>>(input: S) -> Result<Il2CppString, Il2CppError> {
        let res = Il2CppString(null());
        let ffi_str = widestring::U16CString::from_str(input).unwrap();
        res.create_string(ffi_str.as_ptr())
    }
}

impl Display for Il2CppString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            let ptr = &self.first_char();
            let array = std::slice::from_raw_parts(ptr, self.len() as usize);
            match String::from_utf16(&array) {
                Ok(string) => write!(f, "{}", string),
                Err(e) => write!(f, "{}", e),
            }
        }
    }
}

#[ffi_type]
pub struct Il2CppArray;

impl Il2CppArray {
    pub fn monitor(&self) -> *const c_void {
        unsafe { *((self.0.byte_offset(8)) as *const *const c_void) }
    }
    pub fn bounds(&self) -> *const c_void {
        unsafe { *((self.0.byte_offset(16)) as *const *const c_void) }
    }
    pub fn len(&self) -> usize {
        unsafe { *((self.0.byte_offset(24)) as *const usize) }
    }
    fn first_item_ptr(&self) -> *const c_void {
        unsafe { self.0.byte_offset(32) }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get<T>(&self, i: usize) -> &T {
        let size = std::mem::size_of::<T>();
        unsafe { &*((self.first_item_ptr().add(i * size)) as *const T) }
    }
    pub fn get_mut<T>(&mut self, i: usize) -> &mut T {
        let size = std::mem::size_of::<T>();
        unsafe { &mut *((self.first_item_ptr().add(i * size)) as *mut T) }
    }
    pub fn to_vec<T: Clone>(self) -> Vec<T> {
        unsafe {
            std::slice::from_raw_parts(self.first_item_ptr() as *const T, self.len()).to_vec()
        }
    }
}

#[ffi_type]
pub struct List;

impl List {
    pub fn monitor(&self) -> *const c_void {
        unsafe { *((self.0.byte_offset(8)) as *const *const c_void) }
    }
    pub fn items(&self) -> Il2CppArray {
        unsafe { Il2CppArray(*((self.0.byte_offset(16)) as *const *const c_void)) }
    }
    pub fn size(&self) -> i32 {
        unsafe { *((self.0.byte_offset(24)) as *const i32) }
    }
    pub fn to_vec<T: Clone>(self) -> Vec<T> {
        unsafe {
            let items = self.items();
            std::slice::from_raw_parts(items.first_item_ptr() as *const T, self.size() as usize)
                .to_vec()
        }
    }
}

#[il2cpp_ffi_ref_type("System.Type")]
struct System_Type;

impl System_Type {
    #[il2cpp_method(name = "GetTypeFromHandle", args = ["System.RuntimeTypeHandle"])]
    pub fn get_type_from_handle(ty: Il2CppType) -> System_Type {}
}

#[il2cpp_ffi_ref_type("System.RuntimeType")]
pub struct System_RuntimeType;

impl System_RuntimeType {
    // cs_property!(pub base_type, "get_BaseType", RuntimeType, self);
    #[il2cpp_getter_property(property = "BaseType")]
    pub fn get_base_type(&self) -> System_RuntimeType {}

    #[il2cpp_method(name = "GetField", args = ["string", "System.Reflection.BindingFlags"])]
    fn _get_field(&self, name: Il2CppString, binding_flags: i32) -> System_Reflection_FieldInfo {}

    // pub fn get_field<S: AsRef<str>>(&self, name: S) -> Result<Il2CppField, Il2CppError> {
    //     let ffi_name = Il2CppString::new(&name)?;
    //     match self._get_field(ffi_name, 60) {
    //         Ok(field) => {
    //             if field.0 != std::ptr::null() {
    //                 return Ok(field.get_il2cpp_field());
    //             } else {
    //                 let base_type = self.get_base_type()?;
    //                 let field = base_type._get_field(ffi_name, 60)?;
    //                 if field.0 != std::ptr::null() {
    //                     return Ok(field.get_il2cpp_field());
    //                 }
    //             }
    //         }
    //         Err(_) => {
    //             let base_type = self.get_base_type()?;
    //             let field = base_type._get_field(ffi_name, 60)?;
    //             if field.0 != std::ptr::null() {
    //                 return Ok(field.get_il2cpp_field());
    //             }
    //         }
    //     }

    //     Err(Il2CppError::FieldNotFound {
    //         field_name: name.as_ref().to_string(),
    //         type_name: self.get_il2cpp_type().name().to_string(),
    //     })
    // }

    pub fn get_field<S: AsRef<str>>(&self, name: S) -> Result<Il2CppField, Il2CppError> {
        let ffi_name = Il2CppString::new(&name)?;

        let try_get = |rt: &System_RuntimeType| -> Result<Option<Il2CppField>, Il2CppError> {
            match rt._get_field(ffi_name, 60) {
                Ok(field) if field.0 != std::ptr::null() => Ok(Some(field.get_il2cpp_field())),
                Ok(_) => Ok(None),
                Err(_) => Ok(None),
            }
        };

        if let Some(field) = try_get(self)? {
            return Ok(field);
        }

        let base_type = self.get_base_type()?;
        if let Some(field) = try_get(&base_type)? {
            return Ok(field);
        }

        Err(Il2CppError::FieldNotFound {
            field_name: name.as_ref().to_string(),
            type_name: self.get_il2cpp_type().name().to_string(),
        })
    }

    pub fn from_class(class: Il2CppClass) -> Result<Self, Il2CppError> {
        Ok(Self(
            System_Type::get_type_from_handle(class.byval_arg())?.0,
        ))
    }

    pub fn from_name(name: &str) -> Result<Self, Il2CppError> {
        Self::from_class(get_cached_class(name)?)
    }

    pub fn get_il2cpp_type(&self) -> Il2CppType {
        unsafe { Il2CppType(*((self.0.byte_offset(16)) as *const *const c_void)) }
    }
}

pub trait Il2CppObject {
    fn ffi_name() -> &'static str;
    fn get_class(&self) -> Il2CppClass {
        Il2CppClass(&self as *const _ as *const std::ffi::c_void)
    }
    fn get_class_static() -> Result<Il2CppClass, crate::errors::Il2CppError> {
        crate::get_cached_class(Self::ffi_name())
    }
}

#[il2cpp_ffi_ref_type("System.Reflection.FieldInfo")]
pub struct System_Reflection_FieldInfo;

impl System_Reflection_FieldInfo {
    pub fn get_il2cpp_field(&self) -> Il2CppField {
        unsafe { Il2CppField(*((self.0.byte_offset(24)) as *const *const std::ffi::c_void)) }
    }
}

#[il2cpp_ffi_value_type("System.UInt32")]
pub struct System_UInt32(pub u32);

#[il2cpp_ffi_value_type("System.Int32")]
pub struct System_Int32(pub i32);

#[il2cpp_ffi_value_type("System.UInt64")]
pub struct System_UInt64(pub u64);

#[il2cpp_ffi_value_type("System.Int64")]
pub struct System_Int64(pub i64);

#[il2cpp_ffi_value_type("System.Single")]
pub struct System_Single(pub f32);

#[il2cpp_ffi_value_type("System.Double")]
pub struct System_Double(pub f64);

#[il2cpp_ffi_value_type("System.Boolean")]
pub struct System_Boolean(pub bool);
