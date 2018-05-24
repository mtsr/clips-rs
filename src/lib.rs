pub extern crate clips_sys;
#[macro_use]
extern crate failure;

use failure::Fail;
use std::ffi::{CStr, CString};
use std::fmt;

#[derive(Debug, Fail)]
pub enum ClipsError {
  #[fail(display = "oh no")]
  SomeError,
}

#[derive(Debug)]
pub struct Environment {
  inner: *mut clips_sys::Environment,
}

impl Environment {
  pub fn clear(&mut self) -> Result<(), failure::Error> {
    if unsafe { clips_sys::Clear(self.inner) } {
      Ok(())
    } else {
      Err(ClipsError::SomeError.into())
    }
  }

  pub fn load_from_str(&mut self, string: &str) -> Result<(), failure::Error> {
    if unsafe { clips_sys::LoadFromString(self.inner, string.as_ptr() as *const i8, string.len()) }
    {
      Ok(())
    } else {
      Err(ClipsError::SomeError.into())
    }
  }

  pub fn reset(&mut self) {
    unsafe { clips_sys::Reset(self.inner) };
  }

  pub fn get_fact_list<'a>(&'a self) {
    unsafe {
      let fact = clips_sys::GetNextFact(self.inner, std::ptr::null_mut::<clips_sys::Fact>());
      println!("FACT: {:#?}", fact.as_ref().unwrap());
      let slots = unsafe {
        std::slice::from_raw_parts(
          &fact.as_ref().unwrap().theProposition.contents[0],
          fact.as_ref().unwrap().theProposition.length,
        )
      };
      println!(
        "FACT: {:#?}",
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
      println!("NUM_SLOTS: {:#?}", num_slots);

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
        println!("INDEX: {:#?}", index);

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

      // let fact = clips_sys::GetNextFact(self.inner, std::ptr::null_mut::<clips_sys::Fact>());
      // println!("{:#?}", fact.as_ref().unwrap());

      // let mut value: clips_sys::clipsValue = clips_sys::CLIPSValue {
      //   __bindgen_anon_1: std::mem::zeroed(),
      // };
      // clips_sys::GetFactList(self.inner, &mut value, std::ptr::null_mut());
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
}

pub struct Fact {
  inner: *const clips_sys::Fact,
}

pub fn create_environment() -> Result<Environment, failure::Error> {
  unsafe { clips_sys::CreateEnvironment().as_mut() }
    .ok_or(ClipsError::SomeError.into())
    .map(|environment_data| Environment {
      inner: environment_data,
    })
}

// impl fmt::Debug for Environment {
//   fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//     write!(f, "Environment {{ inner: {:#?} }}", unsafe { *(self.inner) })
//   }
// }

impl Drop for Environment {
  fn drop(&mut self) {
    unsafe { clips_sys::DestroyEnvironment(self.inner) };
  }
}
