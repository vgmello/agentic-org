use super::*;

#[test]
fn exact_integer_token_valuation_matches_doc_87_example() {
    // doc-87 example: $1/M input, $2/M output, $0.50/M cache-read, 1000 in / 200 out / 500 cr / 0 cw = 1650 micro-USD.
    let price = ModelPriceView {
        id: "p1".into(),
        provider: "example".into(),
        model: "m1".into(),
        effective_from: "2026-11-01T00:00:00.000Z".into(),
        currency: "USD".into(),
        input_micro_usd_per_million: Some(1_000_000),
        output_micro_usd_per_million: Some(2_000_000),
        cache_read_micro_usd_per_million: Some(500_000),
        cache_write_micro_usd_per_million: Some(1_250_000),
        created_at: "2026-10-31T12:00:00.000Z".into(),
        created_by_kind: "system".into(),
        created_by_id: None,
    };
    let val = compute_token_cost_micro_usd(Some(1000), Some(200), Some(500), Some(0), &price);
    assert_eq!(val.cost_micro_usd, Some(1650));
    assert_eq!(val.cost_basis, UsageCostBasis::TokenEstimate);
    assert_eq!(val.cost_status, CompletenessStatus::Complete);
    assert_eq!(val.cost_issue, None);

    // Zero cache_write needs no rate; nonzero missing rate yields Partial.
    let partial_price = ModelPriceView {
        cache_write_micro_usd_per_million: None,
        ..price.clone()
    };
    let zero_cw =
        compute_token_cost_micro_usd(Some(1000), Some(200), Some(500), Some(0), &partial_price);
    assert_eq!(zero_cw.cost_status, CompletenessStatus::Complete);
    let nonzero_cw =
        compute_token_cost_micro_usd(Some(1000), Some(200), Some(500), Some(100), &partial_price);
    assert_eq!(nonzero_cw.cost_status, CompletenessStatus::Partial);
    assert_eq!(nonzero_cw.cost_issue, Some("partial_price_rate"));
}
