#![warn(missing_docs)]
//! This crate provides logging functions and configuration for [Bevy](https://bevyengine.org)
//! apps, and automatically configures platform specific log handlers (i.e. WASM or Android).
//!
//! The macros provided for logging are reexported from [`tracing`](https://docs.rs/tracing),
//! and behave identically to it.
//!
//! By default, the [`LogPlugin`] from this crate is included in Bevy's `DefaultPlugins`
//! and the logging macros can be used out of the box, if used.
//!
//! For more fine-tuned control over logging behavior, insert a [`LogSettings`] resource before
//! adding [`LogPlugin`] or `DefaultPlugins` during app initialization.

#[cfg(feature = "trace")]
use std::panic;
#[cfg(feature = "tracing-appender")]
use std::path::PathBuf;

#[cfg(target_os = "android")]
mod android_tracing;

pub mod prelude {
    //! The Bevy Log Prelude.
    #[doc(hidden)]
    pub use bevy_utils::tracing::{
        debug, debug_span, error, error_span, info, info_span, trace, trace_span, warn, warn_span,
    };
}

pub use bevy_utils::tracing::{
    debug, debug_span, error, error_span, info, info_span, trace, trace_span, warn, warn_span,
    Level,
};

use bevy_ecs::prelude::Resource;

use bevy_app::{App, Plugin};
use tracing_log::LogTracer;
#[cfg(feature = "tracing-chrome")]
use tracing_subscriber::fmt::{format::DefaultFields, FormattedFields};
use tracing_subscriber::{prelude::*, registry::Registry, EnvFilter};

/// Adds logging to Apps. This plugin is part of the `DefaultPlugins`. Adding
/// this plugin will setup a collector appropriate to your target platform:
/// * Using [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) by default,
/// logging to `stdout`.
/// * Using [`android_log-sys`](https://crates.io/crates/android_log-sys) on Android,
/// logging to Android logs.
/// * Using [`tracing-wasm`](https://crates.io/crates/tracing-wasm) in WASM, logging
/// to the browser console.
///
/// You can configure this plugin using the resource [`LogSettings`].
/// ```no_run
/// # use bevy_app::{App, NoopPluginGroup as DefaultPlugins};
/// # use bevy_log::LogSettings;
/// # use bevy_utils::tracing::Level;
/// fn main() {
///     App::new()
///         .insert_resource(LogSettings {
///             level: Level::DEBUG,
///             filter: "wgpu=error,bevy_render=info,bevy_ecs=trace".to_string(),
///         })
///         .add_plugins(DefaultPlugins)
///         .run();
/// }
/// ```
///
/// Log level can also be changed using the `RUST_LOG` environment variable.
/// For example, using `RUST_LOG=wgpu=error,bevy_render=info,bevy_ecs=trace cargo run ..`
///
/// It has the same syntax as the field [`LogSettings::filter`], see [`EnvFilter`].
/// If you define the `RUST_LOG` environment variable, the [`LogSettings`] resource
/// will be ignored.
///
/// If you want to setup your own tracing collector, you should disable this
/// plugin from `DefaultPlugins` with [`App::add_plugins_with`]:
/// ```no_run
/// # use bevy_app::{App, NoopPluginGroup as DefaultPlugins};
/// # use bevy_log::LogPlugin;
/// fn main() {
///     App::new()
///         .add_plugins_with(DefaultPlugins, |group| group.disable::<LogPlugin>())
///         .run();
/// }
/// ```
///
/// # Panics
///
/// This plugin should not be added multiple times in the same process. This plugin
/// sets up global logging configuration for **all** Apps in a given process, and
/// rerunning the same initialization multiple times will lead to a panic.
#[derive(Default)]
pub struct LogPlugin;

/// Enum to control how often a new log file will be created
#[cfg(feature = "tracing-appender")]
#[derive(Debug, Clone)]
pub enum Rolling {
    /// Creates a new file every minute and appends the date to the file name
    /// Date format: YYYY-MM-DD-HH-mm
    Minutely,
    /// Creates a new file every hour and appends the date to the file name
    /// Date format: YYYY-MM-DD-HH
    Hourly,
    /// Creates a new file every day and appends the date to the file name
    /// Date format: YYYY-MM-DD
    Daily,
    /// Never creates a new file
    Never,
}

/// Settings to control the `log_to_file` feature
#[cfg(feature = "tracing-appender")]
#[derive(Debug, Clone)]
pub struct FileAppenderSettings {
    /// Controls how often a new file will be created
    rolling: Rolling,
    /// The path of the directory where the log files will be added
    ///
    /// Defaults to the local directory
    path: PathBuf,
    /// The prefix added when creating a file
    prefix: String,
}

#[cfg(feature = "tracing-appender")]
impl Default for FileAppenderSettings {
    fn default() -> Self {
        Self {
            rolling: Rolling::Daily,
            path: PathBuf::from("."),
            prefix: String::from("log"),
        }
    }
}

/// `LogPlugin` settings
#[derive(Resource)]
pub struct LogSettings {
    /// Filters logs using the [`EnvFilter`] format
    pub filter: String,

    /// Filters out logs that are "less than" the given level.
    /// This can be further filtered using the `filter` setting.
    pub level: Level,

    /// ConfigureFileLogging
    #[cfg(feature = "tracing-appender")]
    pub file_appender: FileAppenderSettings,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            filter: "wgpu=error".to_string(),
            level: Level::INFO,
            #[cfg(feature = "tracing-appender")]
            file_appender: FileAppenderSettings::default(),
        }
    }
}

impl Plugin for LogPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "trace")]
        {
            let old_handler = panic::take_hook();
            panic::set_hook(Box::new(move |infos| {
                println!("{}", tracing_error::SpanTrace::capture());
                old_handler(infos);
            }));
        }

        let default_filter = {
            let settings = app.world.get_resource_or_insert_with(LogSettings::default);
            format!("{},{}", settings.level, settings.filter)
        };
        LogTracer::init().unwrap();
        let filter_layer = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new(&default_filter))
            .unwrap();
        let subscriber = Registry::default().with(filter_layer);

        #[cfg(feature = "trace")]
        let subscriber = subscriber.with(tracing_error::ErrorLayer::default());

        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            #[cfg(feature = "tracing-chrome")]
            let chrome_layer = {
                let mut layer = tracing_chrome::ChromeLayerBuilder::new();
                if let Ok(path) = std::env::var("TRACE_CHROME") {
                    layer = layer.file(path);
                }
                let (chrome_layer, guard) = layer
                    .name_fn(Box::new(|event_or_span| match event_or_span {
                        tracing_chrome::EventOrSpan::Event(event) => event.metadata().name().into(),
                        tracing_chrome::EventOrSpan::Span(span) => {
                            if let Some(fields) =
                                span.extensions().get::<FormattedFields<DefaultFields>>()
                            {
                                format!("{}: {}", span.metadata().name(), fields.fields.as_str())
                            } else {
                                span.metadata().name().into()
                            }
                        }
                    }))
                    .build();
                app.world.insert_non_send_resource(guard);
                chrome_layer
            };

            #[cfg(feature = "tracing-tracy")]
            let tracy_layer = tracing_tracy::TracyLayer::new();

            let fmt_layer = tracing_subscriber::fmt::Layer::default();
            #[cfg(feature = "tracing-tracy")]
            let fmt_layer = fmt_layer.with_filter(
                tracing_subscriber::filter::Targets::new().with_target("tracy", Level::ERROR),
            );

            let subscriber = subscriber.with(fmt_layer);

            #[cfg(feature = "tracing-appender")]
            let subscriber = {
                let file_output = {
                    let settings = app.world.get_resource_or_insert_with(LogSettings::default);
                    settings.file_appender.clone()
                };

                let file_appender = tracing_appender::rolling::RollingFileAppender::new(
                    match file_output.rolling {
                        Rolling::Minutely => tracing_appender::rolling::Rotation::MINUTELY,
                        Rolling::Hourly => tracing_appender::rolling::Rotation::HOURLY,
                        Rolling::Daily => tracing_appender::rolling::Rotation::DAILY,
                        Rolling::Never => tracing_appender::rolling::Rotation::NEVER,
                    },
                    file_output.path,
                    file_output.prefix,
                );

                let (non_blocking, worker_guard) = tracing_appender::non_blocking(file_appender);
                let file_fmt_layer = tracing_subscriber::fmt::Layer::default()
                    .with_ansi(false)
                    .with_writer(non_blocking);
                // We need to keep this somewhere so it doesn't get dropped. If it gets dropped then it will silently stop writing to the file
                app.insert_resource(worker_guard);

                subscriber.with(file_fmt_layer)
            };

            #[cfg(feature = "tracing-chrome")]
            let subscriber = subscriber.with(chrome_layer);
            #[cfg(feature = "tracing-tracy")]
            let subscriber = subscriber.with(tracy_layer);

            bevy_utils::tracing::subscriber::set_global_default(subscriber)
                .expect("Could not set global default tracing subscriber. If you've already set up a tracing subscriber, please disable LogPlugin from Bevy's DefaultPlugins");
        }

        #[cfg(target_arch = "wasm32")]
        {
            console_error_panic_hook::set_once();
            let subscriber = subscriber.with(tracing_wasm::WASMLayer::new(
                tracing_wasm::WASMLayerConfig::default(),
            ));
            bevy_utils::tracing::subscriber::set_global_default(subscriber)
                .expect("Could not set global default tracing subscriber. If you've already set up a tracing subscriber, please disable LogPlugin from Bevy's DefaultPlugins");
        }

        #[cfg(target_os = "android")]
        {
            let subscriber = subscriber.with(android_tracing::AndroidLayer::default());
            bevy_utils::tracing::subscriber::set_global_default(subscriber)
                .expect("Could not set global default tracing subscriber. If you've already set up a tracing subscriber, please disable LogPlugin from Bevy's DefaultPlugins");
        }
    }
}
