fn main() {
  println!("cargo:rerun-if-changed=../../config/rules");
  println!("cargo:rerun-if-changed=../../config/policies");
}
