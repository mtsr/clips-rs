pub extern crate clips_sys;
#[macro_use]
extern crate failure;

use failure::Fail;
use std::ffi::{CStr, CString};
use std::fmt;
use std::marker;

#[derive(Debug, Fail)]
pub enum ClipsError {
  #[fail(display = "oh no")]
  SomeError,
}

#[derive(Debug)]
pub struct Environment {
  raw: *mut clips_sys::Environment,
}

impl Environment {
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

  pub fn run(&mut self, limit: i64) {
    unsafe { clips_sys::Run(self.raw, limit) };
  }

  pub fn get_fact_list<'env>(&'env self) {
    unsafe {
      let fact = clips_sys::GetNextFact(self.raw, std::ptr::null_mut::<clips_sys::Fact>());
      println!("FACT: {:#?}", fact.as_ref().unwrap());
      let slots = unsafe {
        std::slice::from_raw_parts(
          &fact.as_ref().unwrap().theProposition.contents[0],
          fact.as_ref().unwrap().theProposition.length,
        )
      };
      println!(
        "SLOTS: {:#?}",
        slots
          .iter()
          .map(|slot| slot.__bindgen_anon_1.header.as_ref().as_ref())
          .collect::<Vec<_>>()
      );

      let mut slot_names_result: clips_sys::clipsValue = clips_sys::CLIPSValue {
        __bindgen_anon_1: std::mem::zeroed(),
      };
      clips_sys::FactSlotNames(fact, &mut slot_names_result);
      println!("SLOT_NAMES: {:#?}", slot_names_result);

      let num_slots = slot_names_result
        .__bindgen_anon_1
        .multifieldValue
        .as_ref()
        .as_ref()
        .unwrap()
        .length;
      println!("NUM_SLOT_NAMES: {:#?}", num_slots);

      let slot_names = unsafe {
        std::slice::from_raw_parts(
          &slot_names_result
            .__bindgen_anon_1
            .multifieldValue
            .as_ref()
            .as_ref()
            .unwrap()
            .contents[0],
          num_slots,
        )
      };

      (0..num_slots).for_each(|index| {
        println!("SLOT_NAME_INDEX: {:#?}", index);

        let slot_name = slot_names[index];
        println!("SLOT_NAME: {:#?}", slot_name);

        println!(
          "LEXEME: {:#?}",
          slot_name
            .__bindgen_anon_1
            .lexemeValue
            .as_ref()
            .as_ref()
            .unwrap()
        );
        println!(
          "LEXEME CONTENTS: {:#?}",
          slot_name
            .__bindgen_anon_1
            .lexemeValue
            .as_ref()
            .as_ref()
            .unwrap()
            .contents
        );
        println!(
          "LEXEME CONTENTS VALUE: {:#?}",
          CStr::from_ptr(
            slot_name
              .__bindgen_anon_1
              .lexemeValue
              .as_ref()
              .as_ref()
              .unwrap()
              .contents
              .as_ref()
              .unwrap()
          )
        );

        // println!("{:#?}", slot_name.contents[0]);
        // slot_name = slot_name.contents[0]
        //   .__bindgen_anon_1
        //   .lexemeValue
        //   .as_ref()
        //   .as_ref()
        //   .unwrap()
        //   .next();

        // let slot_value = std::ptr::null_mut::<clips_sys::CLIPSValue>();
        // clips_sys::GetFactSlot(fact, slot_name, slot_value);
      });

      // let fact = clips_sys::GetNextFact(self.raw, std::ptr::null_mut::<clips_sys::Fact>());
      // println!("{:#?}", fact.as_ref().unwrap());

      // let mut value: clips_sys::clipsValue = clips_sys::CLIPSValue {
      //   __bindgen_anon_1: std::mem::zeroed(),
      // };
      // clips_sys::GetFactList(self.raw, &mut value, std::ptr::null_mut());
      // println!(
      //   "{:#?}",
      //   value
      //     .__bindgen_anon_1
      //     .multifieldValue
      //     .as_ref()
      //     .as_ref()
      //     .unwrap() // .contents[0]
      //               // .__bindgen_anon_1
      //               // .factValue
      //               // .as_ref()
      //               // .as_ref()
      //               // .unwrap()
      // );
    }
  }

  pub fn get_instance_iter(&self) -> impl Iterator<Item = Instance> {
    InstanceIterator {
      env: self,
      current: std::ptr::null_mut::<clips_sys::Instance>(),
    }
  }
}

pub struct InstanceIterator<'env> {
  env: &'env Environment,
  current: *mut clips_sys::instance,
}

impl<'env> Iterator for InstanceIterator<'env> {
  type Item = Instance<'env>;

  fn next(&mut self) -> Option<Self::Item> {
    self.current = unsafe { clips_sys::GetNextInstance(self.env.raw, self.current) };

    if (self.current.is_null()) {
      return None;
    };

    Some(Instance {
      raw: self.current,
      _marker: marker::PhantomData,
    })
  }
}

#[derive(Debug)]
pub struct Fact {
  raw: *const clips_sys::Fact,
}

#[derive(Debug)]
pub struct Instance<'env> {
  raw: *mut clips_sys::Instance,
  _marker: marker::PhantomData<&'env Environment>,
}

impl<'env> Instance<'env> {
  pub fn get_name(&'env self) -> &'env str {
    unsafe {
      CStr::from_ptr(
        (*self.raw)
          .name
          .as_ref()
          .unwrap()
          .contents
          .as_ref()
          .unwrap(),
      ).to_str()
        .unwrap()
    }
  }

  pub fn get_slot_names(&'env self) -> Vec<String> {
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
      .map(|slot| slot.get_name().to_owned())
      .collect::<Vec<_>>()
  }
}

#[derive(Debug)]
pub struct InstanceSlot<'inst> {
  raw: &'inst clips_sys::InstanceSlot,
  _marker: marker::PhantomData<&'inst Instance<'inst>>,
}

impl<'inst> InstanceSlot<'inst> {
  pub fn get_name(&self) -> &str {
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
      ).to_str()
        .unwrap()
    }
  }
}

pub fn create_environment() -> Result<Environment, failure::Error> {
  unsafe { clips_sys::CreateEnvironment().as_mut() }
    .ok_or(ClipsError::SomeError.into())
    .map(|environment_data| Environment {
      raw: environment_data,
    })
}

// impl fmt::Debug for Environment {
//   fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//     write!(f, "Environment {{ raw: {:#?} }}", unsafe { *(self.raw) })
//   }
// }

impl Drop for Environment {
  fn drop(&mut self) {
    unsafe { clips_sys::DestroyEnvironment(self.raw) };
  }
}
