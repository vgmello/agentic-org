use org_office::EngineChoice;
use org_office::TestOffice;
use org_proto::search::{SearchEntityType, SearchQueryParams, SearchQueryResult, SearchSource};
use org_proto::tasks::TasksListParams;
use org_proto::Outcome;
use serde_json::json;

use super::*;

fn request(id: u64, params: serde_json::Value) -> Request {
    Request {
        id,
        method: SearchQuery::METHOD.to_owned(),
        principal: None,
        params,
    }
}

fn code(response: &Response) -> Option<ErrorCode> {
    match &response.outcome {
        Outcome::Error(error) => Some(error.code),
        Outcome::Result(_) => None,
    }
}

fn answer(response: &Response) -> SearchQueryResult {
    match &response.outcome {
        Outcome::Result(value) => serde_json::from_value(value.clone()).expect("a search result"),
        Outcome::Error(error) => panic!("refused: {error:?}"),
    }
}

#[test]
fn search_query_answers_a_bound_member_on_both_engines() {
    for engine in [
        EngineChoice::Memory,
        EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()),
    ] {
        let office = Office::for_test(TestOffice { engine });
        let owner = office.owner_member_id().to_owned();
        let params = SearchQueryParams {
            q: "hr".into(),
            ..SearchQueryParams::default()
        };
        let asked = Request::new::<SearchQuery>(1, &params).unwrap();
        let found = answer(&dispatch_search_rpc(&office, Some(&owner), &asked).unwrap());
        assert!(found
            .hits
            .iter()
            .any(|hit| hit.entity_type == SearchEntityType::Member));
        assert_eq!(
            found.coverage.searched,
            [SearchSource::Shared, SearchSource::Messages]
        );
        let private = request(2, json!({"q": "plan", "types": ["private_note"]}));
        let private = answer(&dispatch_search_rpc(&office, Some(&owner), &private).unwrap());
        assert_eq!(private.coverage.searched, [SearchSource::PrivateNotes]);
        // A query of no word at all answers nothing, never an error.
        let nothing = request(3, json!({"q": "*()"}));
        let nothing = answer(&dispatch_search_rpc(&office, Some(&owner), &nothing).unwrap());
        assert_eq!((nothing.total, nothing.hits.len()), (0, 0));
    }
}

#[test]
fn search_query_refuses_the_unbound_the_unknown_and_malformed_params() {
    let office = Office::for_test(TestOffice::default());
    let owner = office.owner_member_id().to_owned();
    let valid = request(1, json!({"q": "hr"}));
    let unbound = dispatch_search_rpc(&office, None, &valid).unwrap();
    assert_eq!(code(&unbound), Some(ErrorCode::NotAuthorized));
    let stranger = "0195a000-0000-7000-8000-0000000000ff";
    let unknown = dispatch_search_rpc(&office, Some(stranger), &valid).unwrap();
    assert_eq!(code(&unknown), Some(ErrorCode::NotAuthorized));
    for params in [
        json!({"q": "hr", "sql": "1=1"}),
        json!({"q": "hr", "types": ["document"]}),
        json!({"q": "hr", "limit": -1}),
        json!({"types": ["task"]}),
        json!("hr"),
    ] {
        let refused = dispatch_search_rpc(&office, Some(&owner), &request(4, params.clone()));
        assert_eq!(
            code(&refused.unwrap()),
            Some(ErrorCode::InvalidRequest),
            "{params}"
        );
    }
    let mixed = request(5, json!({"q": "hr", "types": ["task", "private_decision"]}));
    let mixed = dispatch_search_rpc(&office, Some(&owner), &mixed).unwrap();
    assert_eq!(code(&mixed), Some(ErrorCode::InvalidRequest));
    // Another method is not this dispatcher's.
    let other =
        Request::new::<org_proto::tasks::TasksList>(6, &TasksListParams::default()).unwrap();
    assert!(dispatch_search_rpc(&office, Some(&owner), &other).is_none());
}
