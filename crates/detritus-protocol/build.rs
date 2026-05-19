#![allow(missing_docs)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let mut prost = tonic_build::Config::new();
    prost.protoc_executable(protoc);

    let protos = [
        "proto/opentelemetry/proto/collector/logs/v1/logs_service.proto",
        "proto/opentelemetry/proto/logs/v1/logs.proto",
        "proto/opentelemetry/proto/common/v1/common.proto",
        "proto/opentelemetry/proto/resource/v1/resource.proto",
    ];
    let includes = ["proto"];

    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos_with_config(prost, &[protos[0]], &includes)?;

    println!("cargo:rerun-if-changed=proto/VERSION");
    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    Ok(())
}
