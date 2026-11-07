// make user 先构建独立的用户态 ELF，再由内核编译阶段嵌入。
pub static USER_ELF: &[u8] =
    include_bytes!("../user/target/riscv64gc-unknown-none-elf/release/lab5-user");
