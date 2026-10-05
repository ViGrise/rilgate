use blake2::{Blake2b512, Digest};

#[derive(Debug, Clone)]
pub struct Blake2Generator {
  data: [u8; 64],
  index: usize,
}

impl Blake2Generator {
  pub fn new(seed: &[u8], nonce: u32) -> Self {
    let mut data = [0u8; 64];
    let size = seed.len().min(60);
    data[..size].copy_from_slice(&seed[..size]);
    data[60..64].copy_from_slice(&nonce.to_le_bytes());

    Self { data, index: 64 }
  }

  fn refill(&mut self) {
    let digest = Blake2b512::digest(self.data);
    self.data.copy_from_slice(&digest);
    self.index = 0;
  }

  pub fn byte(&mut self) -> u8 {
    if self.index >= 64 {
      self.refill();
    }

    let value = self.data[self.index];
    self.index += 1;
    value
  }

  pub fn u32(&mut self) -> u32 {
    if self.index + 4 > 64 {
      self.refill();
    }

    let value = u32::from_le_bytes(
      self.data[self.index..self.index + 4]
        .try_into()
        .expect("4 bytes"),
    );

    self.index += 4;
    value
  }
}
