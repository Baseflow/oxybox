use std::io::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    let proto_roots = [
        "protos", 
        "protos/prometheus/prompb", 
    ];

    let protos_to_compile = vec![
        "prometheus/prompb/remote.proto", 
    ];

    let mut config = prost_build::Config::new();

    config.disable_comments(["."]);
    config.protoc_arg("--experimental_allow_proto3_optional");

    let includes: Vec<PathBuf> = proto_roots.iter().map(PathBuf::from).collect();

    let out_dir = PathBuf::from("src/proto_generated");
    std::fs::create_dir_all(&out_dir)?;

    config.out_dir(&out_dir);

    config.compile_protos(&protos_to_compile, &includes)?;

    Ok(())
}
