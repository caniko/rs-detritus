#![allow(missing_docs)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: build scripts run single-threaded for this package before code
    // generation starts, so setting PROTOC here cannot race package code.
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

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
        .compile_protos(&[protos[0]], &includes)?;

    println!("cargo:rerun-if-changed=proto/VERSION");
    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    Ok(())
}
