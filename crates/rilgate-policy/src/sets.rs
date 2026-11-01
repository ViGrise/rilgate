use crate::{AsnSets, IpSets};
#[derive(Debug, Default, Clone)]
pub struct PolicySets {
  pub ip: IpSets,
  pub asn: AsnSets,
}
