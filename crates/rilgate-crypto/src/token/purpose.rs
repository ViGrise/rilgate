#[derive(Clone, Copy)]
pub enum TokenPurpose {
  Presentation,
  Challenge,
  Receipt,
  Auth,
}

impl TokenPurpose {
  pub(super) fn domain(self) -> &'static [u8] {
    match self {
      Self::Presentation => b"rilgate/v1/presentation\0",
      Self::Challenge => b"rilgate/v1/challenge\0",
      Self::Receipt => b"rilgate/v1/receipt\0",
      Self::Auth => b"rilgate/v1/auth\0",
    }
  }
}
