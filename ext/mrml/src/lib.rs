use std::ffi::c_void;
use std::os::raw::{c_char, c_long};
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::slice;
use std::{mem, result};

use mrml::mjml::Mjml;
use mrml::prelude::print::Printable;
use mrml::prelude::render::RenderOptions;

use rb_sys::{
  rb_cObject, rb_data_type_struct__bindgen_ty_1, rb_data_type_t,
  rb_data_typed_object_wrap, rb_define_class_under, rb_define_method,
  rb_define_module, rb_define_singleton_method, rb_eTypeError, rb_exc_new,
  rb_exc_raise, rb_intern, rb_obj_class, rb_utf8_str_new, rb_const_get,
  rb_undef_alloc_func, ruby_value_type, size_t, VALUE, Qnil, RB_TYPE,
  RSTRING_LEN, RSTRING_PTR, RTYPEDDATA_GET_DATA, RTYPEDDATA_P,
  RTYPEDDATA_TYPE
};

#[derive(Clone)]
struct Template {
  res: Mjml
}

impl Template {
  fn new(input: &str) -> Result<Self, String> {
    mrml::parse(input)
      .map(|output| Self { res: output.element })
      .map_err(|ex| ex.to_string())
  }

  fn from_json(input: &str) -> Result<Self, String> {
    serde_json::from_str::<Mjml>(input)
      .map(|res| Self { res })
      .map_err(|ex| ex.to_string())
  }

  fn get_title(&self) -> Option<String> {
    self.res.get_title()
  }

  fn get_preview(&self) -> Option<String> {
    self.res.get_preview()
  }

  fn to_mjml(&self) -> Result<String, String> {
    self.res.print_dense().map_err(|ex| ex.to_string())
  }

  fn to_json(&self) -> Result<String, String> {
    serde_json::to_string(&self.res).map_err(|ex| ex.to_string())
  }

  fn to_html(&self) -> Result<String, String> {
    self.res.render(&RenderOptions::default()).map_err(|ex| ex.to_string())
  }
}

const TEMPLATE_TYPE_NAME: &[u8] = b"MRML::Template\0";

enum AppError {
  Mrml(String),
  Type(String)
}

impl From<String> for AppError {
  fn from(error: String) -> Self {
    Self::Mrml(error)
  }
}

impl From<std::str::Utf8Error> for AppError {
  fn from(_error: std::str::Utf8Error) -> Self {
    Self::Type("input string must be valid UTF-8".to_string())
  }
}

struct TemplateDataType(rb_data_type_t);

// Ruby treats rb_data_type_t as immutable process-wide metadata after init.
unsafe impl Sync for TemplateDataType {}

static TEMPLATE_TYPE: TemplateDataType = TemplateDataType(
  rb_data_type_t {
    wrap_struct_name: TEMPLATE_TYPE_NAME.as_ptr() as *const c_char,
    function: rb_data_type_struct__bindgen_ty_1 {
      dmark: None,
      dfree: Some(template_free),
      dsize: Some(template_size),
      dcompact: None,
      reserved: [ptr::null_mut(); 1]
    },
    parent: ptr::null(),
    data: ptr::null_mut(),
    flags: 0
  }
);

unsafe extern "C" fn template_free(ptr: *mut c_void) {
  if !ptr.is_null() {
    drop(Box::from_raw(ptr as *mut Template));
  }
}

unsafe extern "C" fn template_size(_ptr: *const c_void) -> size_t {
  mem::size_of::<Template>() as size_t
}

fn cstr(bytes: &'static [u8]) -> *const c_char {
  bytes.as_ptr() as *const c_char
}

type RubyMethod = unsafe extern "C" fn() -> VALUE;
type RubyMethod0 = unsafe extern "C" fn(VALUE) -> VALUE;
type RubyMethod1 = unsafe extern "C" fn(VALUE, VALUE) -> VALUE;

unsafe fn method0(func: RubyMethod0) -> Option<RubyMethod> {
  Some(mem::transmute::<RubyMethod0, RubyMethod>(func))
}

unsafe fn method1(func: RubyMethod1) -> Option<RubyMethod> {
  Some(mem::transmute::<RubyMethod1, RubyMethod>(func))
}

fn template_type() -> *const rb_data_type_t {
  &TEMPLATE_TYPE.0
}

unsafe fn module() -> VALUE {
  rb_define_module(cstr(b"MRML\0"))
}

unsafe fn error_class() -> VALUE {
  rb_const_get(module(), rb_intern(cstr(b"Error\0")))
}

unsafe fn raise_error(class: VALUE, message: String) -> ! {
  let exception = rb_exc_new(
    class,
    message.as_ptr() as *const c_char,
    message.len() as c_long
  );

  drop(message);
  rb_exc_raise(exception)
}

unsafe fn raise_mrml_error(message: String) -> ! {
  raise_error(error_class(), message)
}

unsafe fn raise_type_error(message: String) -> ! {
  raise_error(rb_eTypeError, message)
}

unsafe fn ruby_callback<F>(func: F) -> VALUE
where
  F: FnOnce() -> result::Result<VALUE, AppError>
{
  match panic::catch_unwind(AssertUnwindSafe(func)) {
    Ok(Ok(value)) => value,
    Ok(Err(AppError::Mrml(ex))) => raise_mrml_error(ex),
    Ok(Err(AppError::Type(ex))) => raise_type_error(ex),
    Err(_) => raise_mrml_error("internal panic in MRML native extension".to_string())
  }
}

unsafe fn with_ruby_str<T, F>(value: VALUE, func: F) -> result::Result<T, AppError>
where
  F: FnOnce(&str) -> result::Result<T, AppError>
{
  if RB_TYPE(value) != ruby_value_type::RUBY_T_STRING {
    return Err(AppError::Type("wrong argument type".to_string()));
  }

  let ptr = RSTRING_PTR(value);
  let len = RSTRING_LEN(value) as usize;
  let bytes = slice::from_raw_parts(ptr as *const u8, len);
  let input = std::str::from_utf8(bytes)?;
  let result = func(input);

  rb_sys::rb_gc_guard!(value);

  result
}

unsafe fn string_to_value(value: String) -> VALUE {
  rb_utf8_str_new(value.as_ptr() as *const c_char, value.len() as c_long)
}

unsafe fn wrap_template(class: VALUE, template: Template) -> VALUE {
  let ptr = Box::into_raw(Box::new(template)) as *mut c_void;
  rb_data_typed_object_wrap(class, ptr, template_type())
}

unsafe fn get_template<'a>(value: VALUE) -> result::Result<&'a Template, AppError> {
  if !RTYPEDDATA_P(value) || RTYPEDDATA_TYPE(value) != template_type() {
    return Err(AppError::Type("wrong argument type".to_string()));
  }

  let ptr = RTYPEDDATA_GET_DATA(value) as *const Template;

  if ptr.is_null() {
    return Err(AppError::Type("uninitialized MRML::Template".to_string()));
  }

  Ok(&*ptr)
}

unsafe extern "C" fn template_new(class: VALUE, input: VALUE) -> VALUE {
  ruby_callback(|| {
    with_ruby_str(input, |input| {
      Template::new(input)
        .map(|template| wrap_template(class, template))
        .map_err(AppError::from)
    })
  })
}

unsafe extern "C" fn template_from_json(class: VALUE, input: VALUE) -> VALUE {
  ruby_callback(|| {
    with_ruby_str(input, |input| {
      Template::from_json(input)
        .map(|template| wrap_template(class, template))
        .map_err(AppError::from)
    })
  })
}

unsafe extern "C" fn template_title(value: VALUE) -> VALUE {
  ruby_callback(|| {
    Ok(match get_template(value)?.get_title() {
      Some(title) => string_to_value(title),
      None => Qnil.into()
    })
  })
}

unsafe extern "C" fn template_preview(value: VALUE) -> VALUE {
  ruby_callback(|| {
    Ok(match get_template(value)?.get_preview() {
      Some(preview) => string_to_value(preview),
      None => Qnil.into()
    })
  })
}

unsafe extern "C" fn template_to_mjml(value: VALUE) -> VALUE {
  ruby_callback(|| {
    get_template(value)?
      .to_mjml()
      .map(|output| string_to_value(output))
      .map_err(AppError::from)
  })
}

unsafe extern "C" fn template_to_json(value: VALUE) -> VALUE {
  ruby_callback(|| {
    get_template(value)?
      .to_json()
      .map(|output| string_to_value(output))
      .map_err(AppError::from)
  })
}

unsafe extern "C" fn template_to_html(value: VALUE) -> VALUE {
  ruby_callback(|| {
    get_template(value)?
      .to_html()
      .map(|output| string_to_value(output))
      .map_err(AppError::from)
  })
}

unsafe extern "C" fn template_clone(value: VALUE) -> VALUE {
  ruby_callback(|| {
    Ok(wrap_template(rb_obj_class(value), get_template(value)?.clone()))
  })
}

#[no_mangle]
pub unsafe extern "C" fn Init_mrml() {
  let module = module();
  let class = rb_define_class_under(module, cstr(b"Template\0"), rb_cObject);
  rb_undef_alloc_func(class);

  rb_define_singleton_method(class, cstr(b"new\0"), method1(template_new), 1);
  rb_define_singleton_method(class, cstr(b"from_json\0"), method1(template_from_json), 1);

  rb_define_method(class, cstr(b"title\0"), method0(template_title), 0);
  rb_define_method(class, cstr(b"preview\0"), method0(template_preview), 0);

  rb_define_method(class, cstr(b"to_mjml\0"), method0(template_to_mjml), 0);
  rb_define_method(class, cstr(b"to_json\0"), method0(template_to_json), 0);
  rb_define_method(class, cstr(b"to_html\0"), method0(template_to_html), 0);

  rb_define_method(class, cstr(b"clone\0"), method0(template_clone), 0);
  rb_define_method(class, cstr(b"dup\0"), method0(template_clone), 0);
}
