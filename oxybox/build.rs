// build.rs
use std::io::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    // Define all directories that serve as roots for import resolution.
    // Order can sometimes matter for ambiguity, but usually not for distinct roots.
    let proto_roots = [
        "protos", // This covers `gogoproto/gogo.proto` and `google/protobuf/descriptor.proto`
        "protos/prometheus/prompb", // This covers `types.proto` when imported by `remote.proto`
    ];

    // List all the .proto files you want `prost-build` to compile.
    // Their paths must be relative to one of the `proto_roots`.
    // Since "protos/prometheus/prompb" is now a root, we can list them simply as "remote.proto" and "types.proto"
    // OR, if you prefer, keep them relative to the main "protos" root:
    let protos_to_compile = vec![
        "prometheus/prompb/remote.proto", // Relative to "protos" root
    ];

    // Configure prost_build
    let mut config = prost_build::Config::new();

    config.disable_comments(&["."]);
    config.protoc_arg("--experimental_allow_proto3_optional");

    // Set the include paths for protoc.
    let includes: Vec<PathBuf> = proto_roots.iter().map(PathBuf::from).collect();
    println!("Prost build includes: {:?}", &includes); // Debug print
                                                       //
                                                       //
    let out_dir = PathBuf::from("src/proto_generated");
    // Ensure the directory exists
    std::fs::create_dir_all(&out_dir)?;
    config.out_dir(&out_dir); // Set the output directory

    // Compile the protos
    config.compile_protos(&protos_to_compile, &includes)?;

    Ok(())
}
