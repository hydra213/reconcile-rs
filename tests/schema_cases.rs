//! Snapshot tests for schema reports.
//!
//! Each test builds two small schemas in memory and compares the JSON report
//! with the file of the same name in `tests/snapshots/`. To accept an
//! intended change, run `cargo insta review` (or set `INSTA_UPDATE=always`)
//! and commit the updated snapshot.

use arrow::datatypes::{DataType, Field, Schema};
use reconcilers::policy::SchemaPolicy;
use reconcilers::report::SchemaReport;
use reconcilers::schema::LogicalSchema;

fn schema(fields: Vec<Field>) -> LogicalSchema {
    LogicalSchema::from_arrow(&Schema::new(fields)).expect("test schemas have unique names")
}

fn baseline() -> LogicalSchema {
    schema(vec![
        Field::new("trade_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int32, true),
        Field::new("price", DataType::Decimal128(18, 4), true),
    ])
}

fn report_json(left: LogicalSchema, right: LogicalSchema, policy: SchemaPolicy) -> String {
    SchemaReport::new(left, right, policy).to_json()
}

#[test]
fn identical_schema_passes() {
    insta::assert_snapshot!(report_json(baseline(), baseline(), SchemaPolicy::Strict));
}

#[test]
fn added_column_warns_under_compatible() {
    let right = schema(vec![
        Field::new("trade_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int32, true),
        Field::new("price", DataType::Decimal128(18, 4), true),
        Field::new("desk", DataType::Utf8, true),
    ]);
    insta::assert_snapshot!(report_json(baseline(), right, SchemaPolicy::Compatible));
}

#[test]
fn added_column_fails_under_strict() {
    let right = schema(vec![
        Field::new("trade_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int32, true),
        Field::new("price", DataType::Decimal128(18, 4), true),
        Field::new("desk", DataType::Utf8, true),
    ]);
    insta::assert_snapshot!(report_json(baseline(), right, SchemaPolicy::Strict));
}

#[test]
fn incompatible_type_fails_under_compatible() {
    let right = schema(vec![
        Field::new("trade_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int32, true),
        Field::new("price", DataType::Float64, true),
    ]);
    insta::assert_snapshot!(report_json(baseline(), right, SchemaPolicy::Compatible));
}

#[test]
fn integer_widening_warns_under_compatible() {
    let right = schema(vec![
        Field::new("trade_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int64, true),
        Field::new("price", DataType::Decimal128(18, 4), true),
    ]);
    insta::assert_snapshot!(report_json(baseline(), right, SchemaPolicy::Compatible));
}

#[test]
fn column_order_does_not_change_the_report() {
    let reordered = schema(vec![
        Field::new("price", DataType::Decimal128(18, 4), true),
        Field::new("trade_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int32, true),
    ]);
    assert_eq!(
        report_json(baseline(), reordered, SchemaPolicy::Strict),
        report_json(baseline(), baseline(), SchemaPolicy::Strict),
    );
}
