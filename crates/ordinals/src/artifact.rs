use super::*;
use serde::{Serialize, Serializer};

#[derive(Serialize, Eq, PartialEq, Deserialize, Debug)]
pub enum Artifact {
  Cenotaph(Cenotaph),
  Runestone(Runestone),
}

impl Artifact {
  pub fn mint(&self) -> Option<RuneId> {
    match self {
      Self::Cenotaph(cenotaph) => cenotaph.mint,
      Self::Runestone(runestone) => runestone.mint,
    }
  }
}

pub fn u128_op_to_string_serialize<S>(x: &Option<u128>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
  if let Some(x) = x {
    return s.serialize_str(&x.to_string());
  } else {
    return s.serialize_str("0");
  }

}

pub fn u128_to_string_serialize<S>(x: &u128, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
  return s.serialize_str(&x.to_string());

}
