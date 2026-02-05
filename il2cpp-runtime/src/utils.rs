use std::{borrow::Cow, ffi::CStr};

pub unsafe fn cstr_to_str(ptr: *const i8) -> Cow<'static, str> {
    unsafe { Cow::Borrowed(CStr::from_ptr(ptr).to_str().unwrap_unchecked()) }
}
