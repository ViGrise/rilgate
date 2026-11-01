fn main() {
  println!("cargo:rerun-if-changed=../../config/rules");
  println!("cargo:rerun-if-changed=../../config/policies");
  println!("cargo:rerun-if-changed=../../config/signals");
}
