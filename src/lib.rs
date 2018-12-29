pub extern crate clips_sys;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate bitflags;

use std::borrow::Cow;
use std::ffi::{CStr, CString};
use std::marker;

pub enum SaveScope {
  Local = clips_sys::SaveScope_LOCAL_SAVE as isize,
  Visible = clips_sys::SaveScope_VISIBLE_SAVE as isize,
}

#[derive(Debug, Fail)]
pub enum ClipsError {
  #[fail(display = "oh no")]
  SomeError,
}

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

  pub fn clear(&mut self) -> Result<(), failure::Error> {
    if unsafe { clips_sys::Clear(self.raw) } {
      Ok(())
    } else {
      Err(ClipsError::SomeError.into())
    }
  }

  pub fn load_from_str(&mut self, string: &str) -> Result<(), failure::Error> {
    if unsafe { clips_sys::LoadFromString(self.raw, string.as_ptr() as *const i8, string.len()) } {
      Ok(())
    } else {
      Err(ClipsError::SomeError.into())
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
    function: &dyn FnMut(&mut Self, &mut UDFContext) -> UDFValue<'static>,
  ) -> Result<(), failure::Error>
where {
    let name = CString::new(name).unwrap();

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
        self.raw,                   // environment pointer
        name.as_ptr() as *const i8, // CString with CLIPS function name to expose this UDF as
        match return_types {
          Some(return_types) => return_types.format().as_ptr() as *const i8,
          None => std::ptr::null(),
        }, // CString with CLIPS return types
        min_args,                   // Min number of arguments that needs to be passed to UDF
        max_args,                   // Max number of arguments that can be passed to UDF
        arg_types.as_ptr(),         // String with required argument types
        Some(udf_handler),          // Wrapper extern C fn that calls Rust Fn
        name.as_ptr() as *const i8, // name of the 'real function' that's purely for debugging
        Box::into_raw(function) as *mut _, // UDF Context contains pointer to Rust Fn for use by handler
      )
    };

    match error {
      clips_sys::AddUDFError_AUE_NO_ERROR => Ok(()),
      clips_sys::AddUDFError_AUE_MIN_EXCEEDS_MAX_ERROR => Err(ClipsError::SomeError.into()),
      clips_sys::AddUDFError_AUE_FUNCTION_NAME_IN_USE_ERROR => Err(ClipsError::SomeError.into()),
      clips_sys::AddUDFError_AUE_INVALID_ARGUMENT_TYPE_ERROR => Err(ClipsError::SomeError.into()),
      clips_sys::AddUDFError_AUE_INVALID_RETURN_TYPE_ERROR => Err(ClipsError::SomeError.into()),
      _ => unimplemented!(),
    }
  }

  pub fn remove_udf(&mut self, name: &str) -> Result<(), failure::Error> {
    let name = CString::new(name).unwrap();
    if unsafe { clips_sys::RemoveUDF(self.raw, name.as_ptr() as *const i8) } {
      Ok(())
    } else {
      Err(ClipsError::SomeError.into())
    }
  }

  fn void_constant(&self) -> *mut clips_sys::CLIPSVoid {
    unsafe { (*self.raw).VoidConstant }
  }

  pub fn save_instances(&mut self, filename: &str, scope: SaveScope) -> i64 {
    let filename = CString::new(filename).unwrap();
    unsafe { clips_sys::SaveInstances(self.raw, filename.as_ptr() as *const i8, scope as u32) }
  }

  pub fn batch_star(&mut self, filename: &str) -> Result<(), failure::Error> {
    let filename = CString::new(filename).unwrap();
    if unsafe { clips_sys::BatchStar(self.raw, filename.as_ptr() as *const i8) } {
      Ok(())
    } else {
      Err(ClipsError::SomeError.into())
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
      as *mut Box<Box<FnMut(&mut Environment, &mut UDFContext) -> UDFValue<'static>>>)
  };
  let mut environment = Environment::from_ptr(raw_environment);
  let mut context = UDFContext {
    raw: raw_context,
    _marker: marker::PhantomData,
  };

  let rust_return_value = closure(&mut environment, &mut context);
  // Set value from clips::UDFValue on clips_sys::UDFValue
  unsafe { (*return_value) }.set_from((&environment, rust_return_value));
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
  type Item = UDFValue<'env>;

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
pub enum ClipsValue {}
#[derive(Debug)]
pub struct ExternalAddress;

// pub union udfValue__bindgen_ty_1 {
//     pub value: *mut ::std::os::raw::c_void,
//     pub header: *mut TypeHeader,
//     pub lexemeValue: *mut CLIPSLexeme,
//     pub floatValue: *mut CLIPSFloat,
//     pub integerValue: *mut CLIPSInteger,
//     pub voidValue: *mut CLIPSVoid,
//     pub multifieldValue: *mut Multifield,
//     pub factValue: *mut Fact,
//     pub instanceValue: *mut Instance,
//     pub externalAddressValue: *mut CLIPSExternalAddress,
//     _bindgen_union_align: u64,
// }

#[derive(Debug)]
pub enum UDFValue<'env> {
  Symbol(Cow<'env, str>),
  Lexeme(Cow<'env, str>),
  Float(f64),
  Integer(i64),
  Void(),
  Multifield(Vec<ClipsValue>),
  Fact(Fact<'env>),
  InstanceName(Cow<'env, str>),
  Instance(Instance<'env>),
  ExternalAddress(ExternalAddress),
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl<'env> From<clips_sys::UDFValue> for UDFValue<'env> {
  fn from(udf_value: clips_sys::UDFValue) -> Self {
    let union = udf_value.__bindgen_anon_1;

    match u32::from(unsafe { (*udf_value.__bindgen_anon_1.header).type_ }) {
      clips_sys::FLOAT_TYPE => unimplemented!("float"),
      clips_sys::INTEGER_TYPE => unimplemented!("integer"),
      clips_sys::SYMBOL_TYPE => {
        let value = unsafe { CStr::from_ptr((*union.lexemeValue).contents) }.to_string_lossy();
        UDFValue::Symbol(value)
      }
      clips_sys::STRING_TYPE => {
        let value = unsafe { CStr::from_ptr((*union.lexemeValue).contents) }.to_string_lossy();
        UDFValue::Lexeme(value)
      }
      clips_sys::MULTIFIELD_TYPE => unimplemented!("multifield"),
      clips_sys::EXTERNAL_ADDRESS_TYPE => unimplemented!("external address"),
      clips_sys::FACT_ADDRESS_TYPE => unimplemented!("fact address"),
      clips_sys::INSTANCE_ADDRESS_TYPE => unimplemented!("instance address"),
      clips_sys::INSTANCE_NAME_TYPE => {
        let value = unsafe { CStr::from_ptr((*union.lexemeValue).contents) }.to_string_lossy();
        UDFValue::InstanceName(value)
      }
      clips_sys::VOID_TYPE => UDFValue::Void(),
      _ => panic!(),
    }
  }
}

trait SetFrom<T> {
  fn set_from(&mut self, T);
}

impl<'env> SetFrom<(&'env Environment, UDFValue<'env>)> for clips_sys::UDFValue {
  fn set_from(&mut self, (env, udf_value): (&'env Environment, UDFValue<'env>)) {
    match udf_value {
      UDFValue::Symbol(symbol) => unimplemented!("Symbol"),
      UDFValue::Lexeme(lexeme) => unimplemented!("Lexeme"),
      UDFValue::Float(float) => unimplemented!("Float"),
      UDFValue::Integer(integer) => unimplemented!("Integer"),
      UDFValue::Void() => self.__bindgen_anon_1.voidValue = env.void_constant(),
      UDFValue::Multifield(values) => unimplemented!("Multifield"),
      UDFValue::Fact(fact) => unimplemented!("Fact"),
      UDFValue::Instance(instance) => unimplemented!("Instance"),
      UDFValue::InstanceName(instance) => unimplemented!("Instance"),
      UDFValue::ExternalAddress(address) => unimplemented!("ExternalAddress"),
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
      CStr::from_ptr(
        (*self.raw)
          .name
          .as_ref()
          .unwrap()
          .contents
          .as_ref()
          .unwrap(),
      )
      .to_str()
      .unwrap()
    }
  }

  pub fn slot_names(&'env self) -> Vec<String> {
    let num_slots = unsafe {
      self
        .raw
        .as_ref()
        .unwrap()
        .cls
        .as_ref()
        .unwrap()
        .instanceSlotCount
    } as usize;

    let slot_addresses =
      unsafe { std::slice::from_raw_parts(self.raw.as_ref().unwrap().slotAddresses, num_slots) };

    slot_addresses
      .iter()
      .map(|slot| InstanceSlot {
        raw: unsafe { slot.as_ref().unwrap() },
        _marker: marker::PhantomData,
      })
      .map(|slot| slot.name().to_owned())
      .collect::<Vec<_>>()
  }
}

#[derive(Debug)]
pub struct InstanceSlot<'inst> {
  raw: &'inst clips_sys::InstanceSlot,
  _marker: marker::PhantomData<&'inst Instance<'inst>>,
}

impl<'inst> InstanceSlot<'inst> {
  pub fn name(&self) -> &str {
    unsafe {
      CStr::from_ptr(
        self
          .raw
          .desc
          .as_ref()
          .unwrap()
          .slotName
          .as_ref()
          .unwrap()
          .name
          .as_ref()
          .unwrap()
          .contents
          .as_ref()
          .unwrap(),
      )
      .to_str()
      .unwrap()
    }
  }
}

pub fn create_environment() -> Result<Environment, failure::Error> {
  unsafe { clips_sys::CreateEnvironment().as_mut() }
    .ok_or_else(|| ClipsError::SomeError.into())
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
