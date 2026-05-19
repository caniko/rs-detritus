//! Curated facade over generated OTLP log protobuf modules.

/// Generated OpenTelemetry Protocol log-service types.
pub mod generated {
    /// Package-root modules that match generated prost paths.
    pub mod opentelemetry {
        /// OTLP protobuf namespace.
        pub mod proto {
            /// OTLP common packages.
            pub mod common {
                /// Version 1 common attribute and value types.
                #[allow(missing_docs)]
                pub mod v1 {
                    #![allow(rustdoc::invalid_html_tags)]
                    include!(concat!(
                        env!("OUT_DIR"),
                        "/opentelemetry.proto.common.v1.rs"
                    ));
                }
            }

            /// OTLP resource packages.
            pub mod resource {
                /// Version 1 resource metadata types.
                #[allow(missing_docs)]
                pub mod v1 {
                    #![allow(rustdoc::invalid_html_tags)]
                    include!(concat!(
                        env!("OUT_DIR"),
                        "/opentelemetry.proto.resource.v1.rs"
                    ));
                }
            }

            /// OTLP log packages.
            pub mod logs {
                /// Version 1 log record and request types.
                #[allow(missing_docs)]
                pub mod v1 {
                    #![allow(rustdoc::invalid_html_tags)]
                    include!(concat!(env!("OUT_DIR"), "/opentelemetry.proto.logs.v1.rs"));
                }
            }

            /// OTLP collector service packages.
            pub mod collector {
                /// OTLP logs collector packages.
                pub mod logs {
                    /// Version 1 logs collector service client and server stubs.
                    #[allow(missing_docs)]
                    pub mod v1 {
                        #![allow(rustdoc::invalid_html_tags)]
                        include!(concat!(
                            env!("OUT_DIR"),
                            "/opentelemetry.proto.collector.logs.v1.rs"
                        ));
                    }
                }
            }
        }
    }
}

/// Common OTLP value and attribute types.
pub mod common {
    pub use super::generated::opentelemetry::proto::common::v1::{
        AnyValue, AnyValue as OtlpAnyValue, ArrayValue, InstrumentationScope, KeyValue,
        KeyValueList, any_value,
    };
}

/// OTLP resource metadata types.
pub mod resource {
    pub use super::generated::opentelemetry::proto::resource::v1::Resource;
}

/// OTLP log record types exposed through a stable Detritus path.
pub mod logs {
    pub use super::generated::opentelemetry::proto::collector::logs::v1::{
        ExportLogsServiceRequest, ExportLogsServiceResponse,
        logs_service_client::LogsServiceClient, logs_service_server::LogsService,
        logs_service_server::LogsServiceServer,
    };
    pub use super::generated::opentelemetry::proto::logs::v1::{
        LogRecord, LogsData, ResourceLogs, ScopeLogs, SeverityNumber,
    };
}
