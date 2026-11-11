//! ReconcileRS: deterministic data reconciliation for Parquet datasets.
//!
//! The crate is organised as a pipeline. Each module has one job, and the
//! three functions at the bottom of this file run the modules in order:
//!
//! - [`compare_schemas`] checks the schemas only.
//! - [`compare_rows`] checks the rows only.
//! - [`compare`] runs the full reconciliation.
//!
//! All three work on data that is already in memory. Reading is a separate
//! step: [`input`] reads Parquet and CSV files from disk or object storage,
//! and [`Dataset::from_batches`] accepts Arrow data from any other source.
//!
//! | Module            | Job                                                          |
//! |-------------------|--------------------------------------------------------------|
//! | [`config`]        | Load and check `reconcile.toml`.                             |
//! | [`dataset`]       | Hold one side of a comparison in memory, from any source.    |
//! | [`input`]         | Read Parquet and CSV files, local or remote, into a dataset. |
//! | [`schema`]        | Describe schemas and list the *facts* that differ.           |
//! | [`plan`]          | Decide which columns are keys, compared or not comparable.   |
//! | [`value`]         | Convert Arrow cells to logical values that can be compared.  |
//! | [`normalize`]     | Apply opt-in rules (Unicode, naive timestamps) to values.    |
//! | [`table`]         | Hold one side's values in memory.                            |
//! | [`tolerance`]     | Decide whether two numbers are close enough.                 |
//! | [`rowdiff`]       | Pair rows by key and list the *facts* that differ.           |
//! | [`canonical`]     | Define the exact bytes of a value, for hashing.              |
//! | [`fingerprint`]   | Hash files, schemas and table content.                       |
//! | [`policy`]        | Turn facts into verdicts (pass / warn / inconclusive / fail).|
//! | [`report`]        | Assemble facts and verdicts into a deterministic report.     |
//! | [`run`]           | Record what is specific to one run: time, paths, file sizes. |
//!
//! Facts and verdicts are kept apart on purpose: `schema` and `rowdiff`
//! never decide whether a difference is acceptable, and `policy` never looks
//! at files.
//!
//! In every comparison the **left** dataset is the baseline (for example the
//! legacy system) and the **right** dataset is the candidate (for example the
//! migrated system).

pub mod canonical;
pub mod config;
pub mod dataset;
pub mod error;
pub mod fingerprint;
pub mod input;
pub mod normalize;
pub mod plan;
pub mod policy;
pub mod report;
pub mod rowdiff;
pub mod run;
pub mod schema;
pub mod table;
pub mod tolerance;
pub mod value;

pub use config::Config;
pub use dataset::Dataset;
pub use error::Error;
use fingerprint::Fingerprint;
use plan::ComparisonPlan;
use policy::SchemaPolicy;
use report::{FingerprintSection, Report, RowSection, SchemaSection, SideFingerprints};
use schema::LogicalSchema;
use table::Table;

/// Compares two schemas and judges every difference under `policy`.
///
/// To compare the schemas of files without decoding their rows, get the
/// schemas from [`input::read_schema`].
pub fn compare_schemas(
    left: &LogicalSchema,
    right: &LogicalSchema,
    policy: SchemaPolicy,
) -> Report {
    Report::schema_only(SchemaSection::new(left, right, policy))
}

/// Compares the rows of two datasets by key.
///
/// The schemas are used, because they decide which columns can be compared,
/// but they are not judged: `schema_policy` in the configuration is ignored
/// and the report has no schema section. Use [`compare`] to judge both.
pub fn compare_rows(config: &Config, left: &Dataset, right: &Dataset) -> Result<Report, Error> {
    let compared = ComparedRows::new(config, left, right)?;
    Ok(Report::rows_only(compared.row_section))
}

/// Compares two datasets in full: their schemas, their rows and their
/// fingerprints, with one overall verdict.
pub fn compare(config: &Config, left: &Dataset, right: &Dataset) -> Result<Report, Error> {
    let compared = ComparedRows::new(config, left, right)?;
    let plan = &compared.plan;

    let schema_section = SchemaSection::new(
        &plan.left_schema,
        &plan.right_schema,
        config.comparison.schema_policy,
    );

    let fingerprint_section = FingerprintSection::new(
        plan.normalization,
        config.fingerprint(),
        SideFingerprints {
            schema: Fingerprint::of_schema(&plan.left_schema),
            content: Fingerprint::of_table(&compared.left_table),
        },
        SideFingerprints {
            schema: Fingerprint::of_schema(&plan.right_schema),
            content: Fingerprint::of_table(&compared.right_table),
        },
    );

    Ok(Report::new(
        schema_section,
        compared.row_section,
        fingerprint_section,
    ))
}

/// Everything the row comparison produces. [`compare_rows`] reports the row
/// section alone; [`compare`] adds the schema verdict and the fingerprints,
/// which need the plan and the tables.
struct ComparedRows {
    plan: ComparisonPlan,
    left_table: Table,
    right_table: Table,
    row_section: RowSection,
}

impl ComparedRows {
    /// Checks the configuration against both schemas and compares the rows.
    fn new(config: &Config, left: &Dataset, right: &Dataset) -> Result<Self, Error> {
        // Checks the configuration against both schemas before touching rows.
        let plan = ComparisonPlan::new(config, &left.schema, &right.schema)?;

        let left_table = Table::load(
            &left.batches,
            &plan.left_columns_to_load,
            &plan.normalization,
        );
        let right_table = Table::load(
            &right.batches,
            &plan.right_columns_to_load,
            &plan.normalization,
        );

        let diff = rowdiff::diff_rows(
            &left_table,
            &right_table,
            &plan.keys,
            &plan.compared_columns,
            &plan.tolerances,
        );
        let row_section = RowSection::new(&plan, &diff, &config.policy, &config.report);

        Ok(Self {
            plan,
            left_table,
            right_table,
            row_section,
        })
    }
}
