pub extern crate clips_sys;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate derive_more;

use std::borrow::Cow;
use std::error::Error;
use std::ffi::{CStr, CString};
use std::marker;
use std::path::Path;

pub enum SaveScope {
  Local = clips_sys::SaveScope_LOCAL_SAVE as isize,
  Visible = clips_sys::SaveScope_VISIBLE_SAVE as isize,
}

#[derive(Debug, Display)]
#[display(fmt = "{}", kind)]
pub struct ClipsError {
  pub kind: ClipsErrorKind,
  source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

#[derive(Debug, Display)]
pub enum ClipsErrorKind {
  #[display(fmt = "Failed to create CLIPS environment")]
  CreateEnvironmentError,
  #[display(fmt = "Failed to clear CLIPS environment")]
  ClearError,
  #[display(fmt = "Failed to load from {}", "0")]
  LoadFromStringError(String),
  #[display(fmt = "Failed to remove User Defined Function")]
  RemoveUDFError(String),
  #[display(fmt = "Failed to batch load from file")]
  BatchStarError(String),
  #[display(fmt = "Failed to load from {}", "0")]
  LoadOpenFileError(String),
  #[display(fmt = "Failed to load from {}", "0")]
  LoadParsingError(String),
  #[display(fmt = "Failed to add UDF {}: Min exceeds max", "0")]
  AddUDFMinExceedsMaxError(String),
  #[display(fmt = "Failed to add UDF {}: Function name in use", "0")]
  AddUDFFunctionNameInUseError(String),
  #[display(fmt = "Failed to add UDF {}: Invalid Argument Type", "0")]
  AddUDFInvalidArgumentTypeError(String),
  #[display(fmt = "Failed to add UDF {}: Invalid Return Type", "0")]
  AddUDFInvalidReturnTypeError(String),
}

impl Error for ClipsError {
  fn source(&self) -> Option<&(dyn Error + 'static)> {
    self
      .source
      .as_ref()
      .map(|boxed| boxed.as_ref() as &(dyn Error + 'static))
  }
}

impl From<ClipsErrorKind> for ClipsError {
  fn from(kind: ClipsErrorKind) -> Self {
    ClipsError { kind, source: None }
  }
}

// impl ClipsError {
//   fn load_error(err: impl Error + Send + Sync + 'static) -> Self {
//     ClipsError {
//       kind: ClipsErrorKind::LoadError,.into()
//       source: Some(Box::new(err)),
//     }
//   }
// }

#[derive(Debug)]
pub struct Environment {
  raw: *mut clips_sys::Environment,
  cleanup: bool,
}

impl Environment {
  pub fn from_ptr(raw: *mut clips_sys::Environment) -> Self {
    Environment {
      raw,
      cleanup: false,
    }
  }

  pub fn clear(&mut self) -> Result<(), ClipsError> {
    if unsafe { clips_sys::Clear(self.raw) } {
      Ok(())
    } else {
      Err(ClipsErrorKind::ClearError.into())
    }
  }

  pub fn load_from_str(&mut self, string: &str) -> Result<(), ClipsError> {
    if unsafe { clips_sys::LoadFromString(self.raw, string.as_ptr() as *const i8, string.len() as u64) } {
      Ok(())
    } else {
      Err(ClipsErrorKind::LoadFromStringError(string.to_owned()).into())
    }
  }

  pub fn load(&mut self, path: &Path) -> Result<(), ClipsError> {
    let path_str = CString::new(path.to_str().unwrap()).unwrap();
    let load_error = unsafe { clips_sys::Load(self.raw, path_str.as_ptr() as *const i8) };

    match load_error {
      x if x == clips_sys::LoadError_LE_NO_ERROR => Ok(()),
      x if x == clips_sys::LoadError_LE_OPEN_FILE_ERROR as u32 => {
        Err(ClipsErrorKind::LoadOpenFileError(path.to_str().unwrap().to_owned()).into())
      }
      x if x == clips_sys::LoadError_LE_PARSING_ERROR as u32 => {
        Err(ClipsErrorKind::LoadParsingError(path.to_str().unwrap().to_owned()).into())
      }
      _ => unreachable!(),
    }
  }

  pub fn reset(&mut self) {
    unsafe { clips_sys::Reset(self.raw) };
  }

  pub fn run(&mut self, limit: i64) -> i64 {
    unsafe { clips_sys::Run(self.raw, limit) }
  }

  pub fn instances_iter(&self) -> impl Iterator<Item = Instance> {
    InstanceIterator {
      env: self,
      current: std::ptr::null_mut::<clips_sys::Instance>(),
    }
  }

  pub fn command_loop(&mut self) {
    unsafe { clips_sys::CommandLoop(self.raw) };
  }

  pub fn route_command(&mut self, command: &str) {
    let command = CString::new(command).unwrap();
    unsafe { clips_sys::RouteCommand(self.raw, command.as_ptr() as *const i8, true) };
  }

  // AddUDFError AddUDF(
  //   Environment *env,
  //   const char *clipsName,
  //   const char *returnTypes,
  //   unsigned short minArgs,
  //   unsigned short maxArgs,
  //   const char *argTypes,
  //   UserDefinedFunction *cfp,
  //   const char *cName,
  //   void *context);

  // AddUDF(env,"e","d",0,0,NULL,EulersNumber,"EulersNumber",NULL);
  pub fn add_udf(
    &mut self,
    name: &str,
    return_types: Option<Type>,
    min_args: u16,
    max_args: u16,
    arg_types: Vec<Type>,
    function: &dyn FnMut(&mut Self, &mut UDFContext) -> ClipsValue<'static>,
  ) -> Result<(), ClipsError>
where {
    let c_name = CString::new(name).unwrap();

    // Double Box because Box<FnMut> is a Trait Object i.e. fat pointer
    let function = Box::new(Box::new(function));

    let arg_types = &CString::new(
      arg_types
        .iter()
        .map(|type_bitflag| -> String { type_bitflag.format() })
        .collect::<Vec<String>>()
        .join(";"),
    )
    .unwrap();

    let error = unsafe {
      clips_sys::AddUDF(
        self.raw,                     // environment pointer
        c_name.as_ptr() as *const i8, // CString with CLIPS function name to expose this UDF as
        match return_types {
          Some(return_types) => return_types.format().as_ptr() as *const i8,
          None => std::ptr::null(),
        }, // CString with CLIPS return types
        min_args,                     // Min number of arguments that needs to be passed to UDF
        max_args,                     // Max number of arguments that can be passed to UDF
        arg_types.as_ptr(),           // String with required argument types
        Some(udf_handler),            // Wrapper extern C fn that calls Rust Fn
        c_name.as_ptr() as *const i8, // name of the 'real function' that's purely for debugging
        Box::into_raw(function) as *mut _, // UDF Context contains pointer to Rust Fn for use by handler
      )
    };

    match error {
      clips_sys::AddUDFError_AUE_NO_ERROR => Ok(()),
      clips_sys::AddUDFError_AUE_MIN_EXCEEDS_MAX_ERROR => {
        Err(ClipsErrorKind::AddUDFMinExceedsMaxError(name.to_owned()).into())
      }
      clips_sys::AddUDFError_AUE_FUNCTION_NAME_IN_USE_ERROR => {
        Err(ClipsErrorKind::AddUDFFunctionNameInUseError(name.to_owned()).into())
      }
      clips_sys::AddUDFError_AUE_INVALID_ARGUMENT_TYPE_ERROR => {
        Err(ClipsErrorKind::AddUDFInvalidArgumentTypeError(name.to_owned()).into())
      }
      clips_sys::AddUDFError_AUE_INVALID_RETURN_TYPE_ERROR => {
        Err(ClipsErrorKind::AddUDFInvalidReturnTypeError(name.to_owned()).into())
      }
      _ => unreachable!(),
    }
  }

  pub fn remove_udf(&mut self, name: &str) -> Result<(), ClipsError> {
    let c_name = CString::new(name).unwrap();
    if unsafe { clips_sys::RemoveUDF(self.raw, c_name.as_ptr() as *const i8) } {
      Ok(())
    } else {
      Err(ClipsErrorKind::RemoveUDFError(name.to_owned()).into())
    }
  }

  fn void_constant(&self) -> *mut clips_sys::CLIPSVoid {
    unsafe { (*self.raw).VoidConstant }
  }

  pub fn save_instances(&mut self, filename: &str, scope: SaveScope) -> i64 {
    let filename = CString::new(filename).unwrap();
    unsafe { clips_sys::SaveInstances(self.raw, filename.as_ptr() as *const i8, scope as u32) }
  }

  pub fn batch_star(&mut self, filename: &str) -> Result<(), ClipsError> {
    let c_filename = CString::new(filename).unwrap();
    if unsafe { clips_sys::BatchStar(self.raw, c_filename.as_ptr() as *const i8) } {
      Ok(())
    } else {
      Err(ClipsErrorKind::BatchStarError(filename.to_owned()).into())
    }
  }
}

// https://stackoverflow.com/questions/32270030/how-do-i-convert-a-rust-closure-to-a-c-style-callback#32270215
extern "C" fn udf_handler(
  raw_environment: *mut clips_sys::Environment,
  raw_context: *mut clips_sys::UDFContext,
  return_value: *mut clips_sys::UDFValue,
) {
  let closure = unsafe {
    &mut *(raw_context.as_ref().unwrap().context
      // Double Box because Box<FnMut> is a Trait Object i.e. fat pointer
      as *mut Box<Box<dyn FnMut(&mut Environment, &mut UDFContext) -> ClipsValue<'static>>>)
  };
  let mut environment = Environment::from_ptr(raw_environment);
  let mut context = UDFContext {
    raw: raw_context,
    _marker: marker::PhantomData,
  };

  let rust_return_value = closure(&mut environment, &mut context);
  // Set value from clips::ClipsValue on clips_sys::UDFValue
  unsafe { *return_value }.set_from((&environment, rust_return_value));
}

pub struct ArgumentIterator<'env> {
  context: &'env UDFContext<'env>,
}

impl<'env> ArgumentIterator<'env> {
  pub fn new(context: &'env UDFContext) -> Self {
    ArgumentIterator { context }
  }
}

impl<'env> Iterator for ArgumentIterator<'env> {
  type Item = ClipsValue<'env>;

  fn next(&mut self) -> Option<Self::Item> {
    // Create empty clips::UDFValue for CLIPS to write to
    let mut out_value: clips_sys::UDFValue = Default::default();

    if self.context.has_next_argument() {
      unsafe {
        // TODO specify argument types
        clips_sys::UDFNextArgument(self.context.raw, Type::all().bits(), &mut out_value);
      }

      // Convert clips_sys::UDFValue into clips::UDFValue
      return Some(out_value.into());
    }

    None
  }
}

#[derive(Debug)]
pub struct UDFContext<'env> {
  raw: *mut clips_sys::UDFContext,
  _marker: marker::PhantomData<&'env Environment>,
}

impl<'env> UDFContext<'env> {
  pub fn argument_iter(&'env self) -> ArgumentIterator<'env> {
    ArgumentIterator::new(self)
  }

  pub fn has_next_argument(&'env self) -> bool {
    unsafe { !(*self.raw).lastArg.is_null() }
  }
}

#[derive(Debug)]
pub struct ExternalAddress;

#[derive(Debug)]
pub enum ClipsValue<'env> {
  Symbol(Cow<'env, str>),
  Lexeme(Cow<'env, str>),
  Float(f64),
  Integer(i64),
  Void(),
  Multifield(Vec<ClipsValue<'env>>),
  Fact(Fact<'env>),
  InstanceName(Cow<'env, str>),
  Instance(Instance<'env>),
  ExternalAddress(ExternalAddress),
}

impl<'env> From<clips_sys::clipsValue> for ClipsValue<'env> {
  fn from(clips_value: clips_sys::clipsValue) -> Self {
    match u32::from(unsafe { (*clips_value.__bindgen_anon_1.header).type_ }) {
      clips_sys::FLOAT_TYPE => {
        let value = unsafe { (*clips_value.__bindgen_anon_1.floatValue).contents };
        ClipsValue::Float(value)
      }
      clips_sys::INTEGER_TYPE => {
        let value = unsafe { (*clips_value.__bindgen_anon_1.integerValue).contents };
        ClipsValue::Integer(value)
      }
      clips_sys::SYMBOL_TYPE => {
        let value = unsafe { CStr::from_ptr((*clips_value.__bindgen_anon_1.lexemeValue).contents) }
          .to_string_lossy();
        ClipsValue::Symbol(value)
      }
      clips_sys::STRING_TYPE => {
        let value = unsafe { CStr::from_ptr((*clips_value.__bindgen_anon_1.lexemeValue).contents) }
          .to_string_lossy();
        ClipsValue::Lexeme(value)
      }
      clips_sys::MULTIFIELD_TYPE => ClipsValue::Multifield(
        (0..unsafe { (*clips_value.__bindgen_anon_1.multifieldValue).length })
          .map(|index| {
            unsafe {
              *(*clips_value.__bindgen_anon_1.multifieldValue)
                .contents
                .get_unchecked(index as usize)
            }
            .into()
          })
          .collect::<Vec<_>>(),
      ),
      clips_sys::EXTERNAL_ADDRESS_TYPE => unimplemented!("external address"),
      clips_sys::FACT_ADDRESS_TYPE => unimplemented!("fact address"),
      clips_sys::INSTANCE_ADDRESS_TYPE => unimplemented!("instance address"),
      clips_sys::INSTANCE_NAME_TYPE => {
        let value = unsafe { CStr::from_ptr((*clips_value.__bindgen_anon_1.lexemeValue).contents) }
          .to_string_lossy();
        ClipsValue::InstanceName(value)
      }
      clips_sys::VOID_TYPE => ClipsValue::Void(),
      _ => {
        println!(
          "{:?}",
          u32::from(unsafe { (*clips_value.__bindgen_anon_1.header).type_ })
        );
        panic!()
      }
    }
  }
}

impl<'env> From<clips_sys::UDFValue> for ClipsValue<'env> {
  fn from(udf_value: clips_sys::UDFValue) -> Self {
    let union = udf_value.__bindgen_anon_1;

    match u32::from(unsafe { (*udf_value.__bindgen_anon_1.header).type_ }) {
      clips_sys::FLOAT_TYPE => {
        let value = unsafe { (*union.floatValue).contents };
        ClipsValue::Float(value)
      }
      clips_sys::INTEGER_TYPE => {
        let value = unsafe { (*union.integerValue).contents };
        ClipsValue::Integer(value)
      }
      clips_sys::SYMBOL_TYPE => {
        let value = unsafe { CStr::from_ptr((*union.lexemeValue).contents) }.to_string_lossy();
        ClipsValue::Symbol(value)
      }
      clips_sys::STRING_TYPE => {
        let value = unsafe { CStr::from_ptr((*union.lexemeValue).contents) }.to_string_lossy();
        ClipsValue::Lexeme(value)
      }
      clips_sys::MULTIFIELD_TYPE => {
        let multifield = unsafe { *union.multifieldValue };
        ClipsValue::Multifield(
          (0..multifield.length)
            .map(|index| unsafe { *multifield.contents.get_unchecked(index as usize) }.into())
            .collect::<Vec<_>>(),
        )
      }
      clips_sys::EXTERNAL_ADDRESS_TYPE => unimplemented!("external address"),
      clips_sys::FACT_ADDRESS_TYPE => unimplemented!("fact address"),
      clips_sys::INSTANCE_ADDRESS_TYPE => unimplemented!("instance address"),
      clips_sys::INSTANCE_NAME_TYPE => {
        let value = unsafe { CStr::from_ptr((*union.lexemeValue).contents) }.to_string_lossy();
        ClipsValue::InstanceName(value)
      }
      clips_sys::VOID_TYPE => ClipsValue::Void(),
      _ => panic!(),
    }
  }
}

trait SetFrom<T> {
  fn set_from(&mut self, T);
}

impl<'env> SetFrom<(&'env Environment, ClipsValue<'env>)> for clips_sys::UDFValue {
  fn set_from(&mut self, (env, clips_value): (&'env Environment, ClipsValue<'env>)) {
    match clips_value {
      ClipsValue::Symbol(_symbol) => unimplemented!("Symbol"),
      ClipsValue::Lexeme(_lexeme) => unimplemented!("Lexeme"),
      ClipsValue::Float(_float) => unimplemented!("Float"),
      ClipsValue::Integer(_integer) => unimplemented!("Integer"),
      ClipsValue::Void() => self.__bindgen_anon_1.voidValue = env.void_constant(),
      ClipsValue::Multifield(_values) => unimplemented!("Multifield"),
      ClipsValue::Fact(_fact) => unimplemented!("Fact"),
      ClipsValue::Instance(_instance) => unimplemented!("Instance"),
      ClipsValue::InstanceName(_instance) => unimplemented!("Instance"),
      ClipsValue::ExternalAddress(_address) => unimplemented!("ExternalAddress"),
    }
  }
}

bitflags! {
    pub struct Type: u32 {
        const FLOAT = clips_sys::CLIPSType_FLOAT_BIT as u32;
        const INTEGER = clips_sys::CLIPSType_INTEGER_BIT as u32;
        const SYMBOL = clips_sys::CLIPSType_SYMBOL_BIT as u32;
        const STRING = clips_sys::CLIPSType_STRING_BIT as u32;
        const MULTIFIELD = clips_sys::CLIPSType_MULTIFIELD_BIT as u32;
        const EXTERNAL_ADDRESS = clips_sys::CLIPSType_EXTERNAL_ADDRESS_BIT as u32;
        const FACT_ADDRESS = clips_sys::CLIPSType_FACT_ADDRESS_BIT as u32;
        const INSTANCE_ADDRESS = clips_sys::CLIPSType_INSTANCE_ADDRESS_BIT as u32;
        const INSTANCE_NAME = clips_sys::CLIPSType_INSTANCE_NAME_BIT as u32;
        const VOID = clips_sys::CLIPSType_VOID_BIT as u32;
        const BOOLEAN = clips_sys::CLIPSType_BOOLEAN_BIT as u32;
        const ANY = 0b0;
    }
}

impl Type {
  fn format(self) -> String {
    if self.is_empty() || self.contains(Self::ANY) {
      return "*".to_owned();
    }

    let mut result = String::with_capacity(12);

    if self.contains(Self::BOOLEAN) {
      result.push('b')
    }
    if self.contains(Self::FLOAT) {
      result.push('d')
    }
    if self.contains(Self::EXTERNAL_ADDRESS) {
      result.push('e')
    }
    if self.contains(Self::FACT_ADDRESS) {
      result.push('f')
    }
    if self.contains(Self::INSTANCE_ADDRESS) {
      result.push('i')
    }
    if self.contains(Self::INTEGER) {
      result.push('l')
    }
    if self.contains(Self::MULTIFIELD) {
      result.push('m')
    }
    if self.contains(Self::INSTANCE_NAME) {
      result.push('n')
    }
    if self.contains(Self::STRING) {
      result.push('s')
    }
    if self.contains(Self::SYMBOL) {
      result.push('y')
    }
    if self.contains(Self::VOID) {
      result.push('v')
    }
    result.shrink_to_fit();
    result
  }
}

pub struct InstanceIterator<'env> {
  env: &'env Environment,
  current: *mut clips_sys::Instance,
}

impl<'env> Iterator for InstanceIterator<'env> {
  type Item = Instance<'env>;

  fn next(&mut self) -> Option<Self::Item> {
    self.current = unsafe { clips_sys::GetNextInstance(self.env.raw, self.current) };

    if self.current.is_null() {
      return None;
    };

    Some(Instance {
      raw: self.current,
      _marker: marker::PhantomData,
    })
  }
}

#[derive(Debug)]
pub struct Fact<'env> {
  raw: *const clips_sys::Fact,
  _marker: marker::PhantomData<&'env Environment>,
}

#[derive(Debug)]
pub struct Instance<'env> {
  raw: *mut clips_sys::Instance,
  _marker: marker::PhantomData<&'env Environment>,
}

impl<'env> Instance<'env> {
  pub fn name(&'env self) -> &'env str {
    unsafe {
      CStr::from_ptr(clips_sys::InstanceName(self.raw))
        .to_str()
        .unwrap()
    }
  }

  pub fn class_name(&'env self) -> &'env str {
    unsafe {
      CStr::from_ptr(clips_sys::DefclassName(clips_sys::InstanceClass(self.raw)))
        .to_str()
        .unwrap()
    }
  }

  fn slot_names_c(&'env self) -> Vec<*const i8> {
    // TODO argument?
    let inherit = true;

    let mut slot_list: clips_sys::clipsValue = Default::default();
    unsafe {
      let class = clips_sys::InstanceClass(self.raw);
      clips_sys::ClassSlots(class, &mut slot_list as *mut clips_sys::clipsValue, inherit)
    };

    let num_slots = unsafe { *slot_list.__bindgen_anon_1.multifieldValue }.length;

    (0..num_slots)
      .map(|index| unsafe {
        (*(*slot_list.__bindgen_anon_1.multifieldValue)
          .contents
          .get_unchecked(index as usize)
          .__bindgen_anon_1
          .lexemeValue)
          .contents
      })
      .collect::<Vec<_>>()
  }

  pub fn slot_names(&'env self) -> Vec<Cow<'env, str>> {
    self
      .slot_names_c()
      .iter()
      .map(|cstr| unsafe { CStr::from_ptr(*cstr) }.to_string_lossy())
      .collect::<Vec<_>>()
  }

  pub fn slots(&'env self) -> Vec<InstanceSlot<'env>> {
    let value: *mut clips_sys::clipsValue = &mut Default::default();

    self
      .slot_names_c()
      .iter()
      .map(|slot_name| {
        unsafe { clips_sys::DirectGetSlot(self.raw, *slot_name, value) };

        InstanceSlot {
          name: unsafe { CStr::from_ptr(*slot_name) }.to_string_lossy(),
          value: unsafe { *value }.into(),
        }
      })
      .map(|slot| slot)
      .collect::<Vec<_>>()
  }
}

#[derive(Debug)]
pub struct InstanceSlot<'env> {
  pub name: Cow<'env, str>,
  pub value: ClipsValue<'env>,
}

pub fn create_environment() -> Result<Environment, ClipsError> {
  unsafe { clips_sys::CreateEnvironment().as_mut() }
    .ok_or_else(|| ClipsErrorKind::CreateEnvironmentError.into())
    .map(|environment_data| Environment {
      raw: environment_data,
      cleanup: true,
    })
}

impl Drop for Environment {
  fn drop(&mut self) {
    if self.cleanup {
      unsafe { clips_sys::DestroyEnvironment(self.raw) };
    }
  }
}
