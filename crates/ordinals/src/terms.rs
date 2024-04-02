use self::artifact::u128_op_to_string_serialize;

use super::*;

#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Copy, Clone, Eq)]
pub struct Terms {
  #[serde(serialize_with = "u128_op_to_string_serialize")]
  pub amount: Option<u128>,
  #[serde(serialize_with = "u128_op_to_string_serialize")]
  pub cap: Option<u128>,
  pub height: (Option<u64>, Option<u64>),
  pub offset: (Option<u64>, Option<u64>),
}
