use std::sync::Arc;

use minijinja::value::{Object, Value};
use minijinja::{Error, ErrorKind, State as TemplateState};
use serde::Serialize;

use crate::engine::store::{Job, Session, State};
use crate::engine::transcript::{Segment, Transcript};

#[derive(Serialize)]
struct ResultView<'a> {
    language: &'a str,
    duration: f64,
    segments: &'a [Segment],
    text: String,
    timestamped: String,
}

#[derive(Serialize)]
struct JobView<'a> {
    #[serde(flatten)]
    job: &'a Job,
    active: bool,
    result: Option<ResultView<'a>>,
}

fn result(transcript: &Transcript) -> ResultView<'_> {
    ResultView {
        language: &transcript.language,
        duration: transcript.duration,
        segments: &transcript.segments,
        text: transcript.text(),
        timestamped: transcript.timestamped(),
    }
}

pub fn job(job: &Job) -> Value {
    Value::from_serialize(JobView { job, active: job.active(), result: job.transcript.as_ref().map(result) })
}

#[derive(Debug)]
pub struct StoreView(pub State);

impl StoreView {
    fn session(&self, value: &Value) -> Result<&Session, Error> {
        let id = value.get_attr("id").ok().filter(|v| !v.is_undefined()).unwrap_or_else(|| value.clone());
        let id = id.as_str().ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "expected a session"))?;
        self.0.sessions.get(id).ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "unknown session"))
    }
}

impl Object for StoreView {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let state = &self.0;
        Some(match key.as_str()? {
            "active" => Value::from_iter(state.active().into_iter().map(job)),
            "batch_done" => Value::from(state.batch_done),
            "batch_total" => Value::from(state.batch_total),
            "batch_progress" => Value::from(state.batch_progress()),
            "sessions" => Value::from_serialize(&state.sessions),
            _ => return None,
        })
    }

    fn call_method(
        self: &Arc<Self>,
        _: &TemplateState<'_, '_>,
        method: &str,
        args: &[Value],
    ) -> Result<Value, Error> {
        let arg = args.first().ok_or_else(|| Error::new(ErrorKind::MissingArgument, method.to_string()))?;
        match method {
            "notes" => {
                let id = self.session(arg)?.id.clone();
                Ok(Value::from_iter(self.0.notes(&id).into_iter().map(job)))
            }
            "title" => Ok(Value::from(self.0.title(self.session(arg)?))),
            "last_activity" => Ok(Value::from(self.0.last_activity(self.session(arg)?))),
            _ => Err(Error::new(ErrorKind::UnknownMethod, method.to_string())),
        }
    }
}
