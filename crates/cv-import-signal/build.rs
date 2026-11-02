// Compiles Signal's vendored protobuf schemas with a pure-Rust compiler
// (protox), so no `protoc` installation is needed on any platform.

fn main() {
    let files = ["proto/backup.proto", "proto/LocalBackup.proto"];
    for file in files {
        println!("cargo:rerun-if-changed={file}");
    }
    let descriptors = protox::compile(files, ["proto"]).expect("valid protobuf schemas");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("generate Rust code from protobuf schemas");
}
