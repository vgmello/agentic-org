//! The API the office server speaks today: `hello`, `organization.*` and `subscribe`.
//! Each handler maps between wire types and the office's own values; the rules live in
//! `org-office`.

use org_office::OfficeError;
use org_proto::{
    ApiError, Empty, ErrorCode, Hello, HelloResult, ListOrganizationsResult, OfficeInfo,
    Organization, OrganizationCreate, OrganizationCreated, OrganizationList, Subscribe,
    API_VERSION,
};

use crate::router::Router;

/// Where the office runs, for display. An office server only runs on this Mac today.
const HOST: &str = "This Mac";

pub fn standard_router() -> Router {
    let mut router = Router::new();
    router
        .on::<Hello>(|context, Empty {}| {
            let office = context.office.info();
            Ok(HelloResult {
                api_version: API_VERSION,
                server_version: env!("CARGO_PKG_VERSION").to_owned(),
                office: OfficeInfo {
                    id: office.id.clone(),
                    name: office.name.clone(),
                    host: HOST.to_owned(),
                },
            })
        })
        .on::<OrganizationCreate>(|context, params| {
            let organization = wire_organization(
                context
                    .office
                    .create_organization(&params.name)
                    .map_err(api_error)?,
            );
            context.events.publish::<OrganizationCreated>(&organization);
            Ok(organization)
        })
        .on::<OrganizationList>(|context, Empty {}| {
            let organizations = context
                .office
                .list_organizations()
                .map_err(api_error)?
                .into_iter()
                .map(wire_organization)
                .collect();
            Ok(ListOrganizationsResult { organizations })
        })
        .on::<Subscribe>(|context, Empty {}| {
            context.events.subscribe(context.connection);
            Ok(Empty {})
        });
    router
}

fn wire_organization(organization: org_office::Organization) -> Organization {
    Organization {
        id: organization.id,
        name: organization.name,
        created_at: organization.created_at,
    }
}

/// Maps an office failure to what a client may see. The cause of a storage failure goes to
/// the server's log only: it can hold paths or SQL.
fn api_error(error: OfficeError) -> ApiError {
    match &error {
        OfficeError::InvalidName(_) => ApiError::new(ErrorCode::InvalidName, error.to_string()),
        OfficeError::AlreadyExists => ApiError::new(ErrorCode::AlreadyExists, error.to_string()),
        OfficeError::InUse | OfficeError::Storage(_) => {
            log_error(&error);
            ApiError::new(ErrorCode::StorageError, error.to_string())
        }
    }
}

fn log_error(error: &dyn std::error::Error) {
    let mut line = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        line.push_str(&format!(": {next}"));
        cause = next.source();
    }
    eprintln!("org-office-server: {line}");
}
