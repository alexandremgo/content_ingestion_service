use chrono::{DateTime, Utc};
use std::str::FromStr;
use typed_builder::TypedBuilder;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::Type)]
#[sqlx(type_name = "source_type", rename_all = "lowercase")]
pub enum SourceType {
    Epub,
}

impl FromStr for SourceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "epub" => Ok(SourceType::Epub),
            _ => Err(format!("Invalid SourceType: {}", s)),
        }
    }
}

// TODO: delete (was maybe solution for enum handling and compile time check with sqlx)
// impl PgHasArrayType for SourceType {
//     fn array_type_info() -> PgTypeInfo {
//         PgTypeInfo::with_name("_source_type")
//     }
// }

#[derive(Debug, Clone, TypedBuilder)]
pub struct SourceMeta {
    #[builder(default=Uuid::new_v4())]
    pub id: Uuid,

    pub user_id: Uuid,

    /// File name received from the user
    pub initial_name: String,

    /// Name of the file saved in the object store
    pub object_store_name: String,

    pub source_type: SourceType,

    #[builder(default=Utc::now())]
    pub added_at: DateTime<Utc>,

    #[builder(default)]
    pub extracted_at: Option<DateTime<Utc>>,
}

// // Could not derive FromRow directly because of `SourceType`
// impl FromRow<'_, PgRow> for SourceMeta {
//     fn from_row(row: &PgRow) -> sqlx::Result<Self> {
//         let source_meta = SourceMeta::builder()
//             .id(row.try_get("id")?)
//             .user_id(row.try_get("user_id")?)
//             .initial_name(row.try_get("initial_name")?)
//             .object_store_name(row.try_get("object_store_name")?)
//             .source_type(
//                 SourceType::from_str(row.try_get("source_type")?).map_err(|_| {
//                     sqlx::Error::TypeNotFound {
//                         type_name: "SourceType".to_string(),
//                     }
//                 })?,
//             )
//             .added_at(row.try_get("added_at")?)
//             .extracted_at(row.try_get("extracted_at")?)
//             .build();

//         Ok(source_meta)
//     }
// }
