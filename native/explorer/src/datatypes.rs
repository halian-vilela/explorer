use chrono::prelude::*;
use polars::prelude::*;
use rustler::resource::ResourceArc;
use rustler::{Atom, Encoder, Env, NifStruct, NifUntaggedEnum, Term};
use std::convert::TryInto;
use std::sync::RwLock;

use std::result::Result;

use crate::atoms;

pub struct ExDataFrameRef(pub RwLock<DataFrame>);
pub struct ExSeriesRef(pub Series);

#[derive(NifStruct)]
#[module = "Explorer.PolarsBackend.DataFrame"]
pub struct ExDataFrame {
    pub resource: ResourceArc<ExDataFrameRef>,
}

#[derive(NifStruct)]
#[module = "Explorer.PolarsBackend.Series"]
pub struct ExSeries {
    pub resource: ResourceArc<ExSeriesRef>,
}

impl ExDataFrameRef {
    pub fn new(df: DataFrame) -> Self {
        Self(RwLock::new(df))
    }
}

impl ExSeriesRef {
    pub fn new(s: Series) -> Self {
        Self(s)
    }
}

impl ExDataFrame {
    pub fn new(df: DataFrame) -> Self {
        Self {
            resource: ResourceArc::new(ExDataFrameRef::new(df)),
        }
    }
}

impl ExSeries {
    pub fn new(s: Series) -> Self {
        Self {
            resource: ResourceArc::new(ExSeriesRef::new(s)),
        }
    }
}

#[derive(NifStruct, Copy, Clone, Debug)]
#[module = "Date"]
pub struct ExDate {
    pub calendar: Atom,
    pub day: u32,
    pub month: u32,
    pub year: i32,
}

impl From<i32> for ExDate {
    fn from(ts: i32) -> Self {
        let seconds = ts * 86_400;
        let dt = NaiveDateTime::from_timestamp(seconds.into(), 0);
        ExDate::from(NaiveDate::from_yo(dt.year(), dt.ordinal()))
    }
}

impl From<ExDate> for i32 {
    fn from(d: ExDate) -> i32 {
        NaiveDate::from_ymd(d.year, d.month, d.day)
            .signed_duration_since(NaiveDate::from_ymd(1970, 1, 1))
            .num_days()
            .try_into()
            .unwrap()
    }
}

impl From<ExDate> for NaiveDate {
    fn from(d: ExDate) -> NaiveDate {
        NaiveDate::from_ymd(d.year, d.month, d.day)
    }
}

impl From<NaiveDate> for ExDate {
    fn from(d: NaiveDate) -> ExDate {
        ExDate {
            calendar: atoms::calendar(),
            day: d.day(),
            month: d.month(),
            year: d.year(),
        }
    }
}

#[derive(NifStruct, Copy, Clone, Debug)]
#[module = "NaiveDateTime"]
pub struct ExDateTime {
    pub calendar: Atom,
    pub day: u32,
    pub month: u32,
    pub year: i32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub microsecond: (u32, u32),
}

impl From<i64> for ExDateTime {
    fn from(ms: i64) -> Self {
        let sign = ms.signum();
        let seconds = match sign {
            -1 => ms / 1_000 - 1,
            _ => ms / 1_000,
        };
        let remainder = match sign {
            -1 => 1_000 + ms % 1_000,
            _ => ms % 1_000,
        };
        let nanoseconds = remainder.abs() * 1_000_000;
        ExDateTime::from(NaiveDateTime::from_timestamp(
            seconds,
            nanoseconds.try_into().unwrap(),
        ))
    }
}

impl From<ExDateTime> for i64 {
    fn from(dt: ExDateTime) -> i64 {
        NaiveDate::from_ymd(dt.year, dt.month, dt.day)
            .and_hms_micro(dt.hour, dt.minute, dt.second, dt.microsecond.0)
            .signed_duration_since(NaiveDate::from_ymd(1970, 1, 1).and_hms(0, 0, 0))
            .num_milliseconds()
    }
}

impl From<ExDateTime> for NaiveDateTime {
    fn from(dt: ExDateTime) -> NaiveDateTime {
        NaiveDate::from_ymd(dt.year, dt.month, dt.day).and_hms_micro(
            dt.hour,
            dt.minute,
            dt.second,
            dt.microsecond.0,
        )
    }
}

impl From<NaiveDateTime> for ExDateTime {
    fn from(dt: NaiveDateTime) -> Self {
        ExDateTime {
            calendar: atoms::calendar(),
            day: dt.day(),
            month: dt.month(),
            year: dt.year(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            microsecond: (dt.timestamp_subsec_micros(), 3),
        }
    }
}

fn encode_date_series<'b>(s: &Series, env: Env<'b>) -> Term<'b> {
    s.date()
        .unwrap()
        .as_date_iter()
        .map(|d| d.map(ExDate::from))
        .collect::<Vec<Option<ExDate>>>()
        .encode(env)
}

fn encode_datetime_series<'b>(s: &Series, env: Env<'b>) -> Term<'b> {
    s.datetime()
        .unwrap()
        .into_iter()
        .map(|d| d.map(ExDateTime::from))
        .collect::<Vec<Option<ExDateTime>>>()
        .encode(env)
}

macro_rules! encode {
    ($s:ident, $env:ident, $convert_function:ident, $out_type:ty) => {
        $s.$convert_function()
            .unwrap()
            .into_iter()
            .map(|item| item)
            .collect::<Vec<Option<$out_type>>>()
            .encode($env)
    };
    ($s:ident, $env:ident, $convert_function:ident) => {
        $s.$convert_function()
            .unwrap()
            .into_iter()
            .map(|item| item)
            .collect::<Vec<Option<$convert_function>>>()
            .encode($env)
    };
}

macro_rules! encode_list {
    ($s:ident, $env:ident, $convert_function:ident, $out_type:ty) => {
        $s.list()
            .unwrap()
            .into_iter()
            .map(|item| item)
            .collect::<Vec<Option<Series>>>()
            .iter()
            .map(|item| {
                item.clone()
                    .unwrap()
                    .$convert_function()
                    .unwrap()
                    .into_iter()
                    .map(|item| item)
                    .collect::<Vec<Option<$out_type>>>()
            })
            .collect::<Vec<Vec<Option<$out_type>>>>()
            .encode($env)
    };
}

impl<'a> Encoder for ExSeriesRef {
    fn encode<'b>(&self, env: Env<'b>) -> Term<'b> {
        let s = &self.0;
        match s.dtype() {
            DataType::Boolean => encode!(s, env, bool),
            DataType::Utf8 => encode!(s, env, utf8, &str),
            DataType::Int32 => encode!(s, env, i32),
            DataType::Int64 => encode!(s, env, i64),
            DataType::UInt32 => encode!(s, env, u32),
            DataType::Float64 => encode!(s, env, f64),
            DataType::Date => encode_date_series(s, env),
            DataType::Datetime(TimeUnit::Milliseconds, None) => encode_datetime_series(s, env),
            DataType::List(t) if t as &DataType == &DataType::UInt32 => {
                encode_list!(s, env, u32, u32)
            }
            dt => panic!("to_list/1 not implemented for {:?}", dt),
        }
    }
}

#[derive(NifUntaggedEnum, Clone, Debug)]
pub enum ExAnyValue {
    Boolean(bool),
    Utf8(String),
    Int32(i32),
    Int64(i64),
    UInt32(u32),
    Float64(f64),
    Datetime(ExDateTime),
    Date(ExDate),
}

impl From<ExAnyValue> for AnyValue<'_> {
    fn from(val: ExAnyValue) -> Self {
        let value = match val {
            ExAnyValue::Boolean(x) => AnyValue::Boolean(x),
            ExAnyValue::Utf8(x) => AnyValue::Utf8Owned(x),
            ExAnyValue::Int32(x) => AnyValue::Int32(x),
            ExAnyValue::Int64(x) => AnyValue::Int64(x),
            ExAnyValue::UInt32(x) => AnyValue::UInt32(x),
            ExAnyValue::Float64(x) => AnyValue::Float64(x),
            ExAnyValue::Datetime(x) => {
                AnyValue::Datetime(i64::from(x), TimeUnit::Milliseconds, &None)
            }
            ExAnyValue::Date(x) => AnyValue::Date(i32::from(x)),
        };
        value
    }
}

impl From<AnyValue<'_>> for ExAnyValue {
    fn from(val: AnyValue) -> Self {
        match val {
            AnyValue::Boolean(x) => ExAnyValue::Boolean(x),
            AnyValue::Utf8(x) => ExAnyValue::Utf8(x.to_string()),
            AnyValue::Utf8Owned(x) => ExAnyValue::Utf8(x),
            AnyValue::Int32(x) => ExAnyValue::Int32(x),
            AnyValue::Int64(x) => ExAnyValue::Int64(x),
            AnyValue::UInt32(x) => ExAnyValue::UInt32(x),
            AnyValue::Float64(x) => ExAnyValue::Float64(x),
            AnyValue::Datetime(x, ..) => ExAnyValue::Datetime(ExDateTime::from(x)),
            AnyValue::Date(x) => ExAnyValue::Date(ExDate::from(x)),
            _ => panic!("unsupported datatype for {:?}", val),
        }
    }
}
