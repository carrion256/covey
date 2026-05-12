use std::sync::Once;

use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
    metadata::LevelFilter,
    span::{Attributes, Id, Record},
    subscriber::Interest,
};

static TRACING: Once = Once::new();

struct CoverageSubscriber;

impl Subscriber for CoverageSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= Level::INFO
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        event.record(&mut NoopVisitor);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.enabled(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::INFO)
    }
}

struct NoopVisitor;

impl Visit for NoopVisitor {
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

pub(crate) fn enable_info_logging() {
    TRACING.call_once(|| {
        let _ = tracing::subscriber::set_global_default(CoverageSubscriber);
    });
}
