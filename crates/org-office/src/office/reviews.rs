//! High-level universal reviews facade on [`Office`] (`TASK-92.1`, `ADR-0026`, `ADR-0028`).

use org_proto::reviews::{
    ReviewCloseParams, ReviewFindingParams, ReviewFindingResult, ReviewInspectParams,
    ReviewInspectResult, ReviewListParams, ReviewListResult, ReviewMutationResult,
    ReviewRequestParams, ReviewResubmitParams, ReviewRoundResult, ReviewRouteParams,
    ReviewRouteResult, ReviewVerdictParams, ReviewVerdictResult,
};
use org_proto::ApiError;

use crate::error::OfficeError;
use crate::reviews::features::ReviewInspectSections;
use crate::reviews::records::{ResolvedCodePin, ReviewEventRecord};
use crate::store::open::review_ops::{
    ReviewCloseOp, ReviewEventsRead, ReviewFindingOpCmd, ReviewInspectRead, ReviewListRead,
    ReviewRequestOp, ReviewResubmitOp, ReviewRouteOp, ReviewVerdictOp,
};
use crate::store::open::{ReadOp, StoreOp};

use super::{storage, Office};

type Answer<T> = Result<Result<T, ApiError>, OfficeError>;

impl Office {
    fn r_read<R: ReadOp<Out = Result<T, ApiError>>, T>(&self, op: R) -> Answer<T> {
        Ok(self.store.read(op).map_err(storage)?.0)
    }
    fn r_run<O: StoreOp<Out = Result<Result<T, ApiError>, org_store::StoreError>>, T>(
        &self,
        op: O,
    ) -> Answer<T> {
        self.store.run(op).map_err(storage)
    }

    pub fn review_request(&self, c: &str, p: ReviewRequestParams) -> Answer<ReviewRoundResult> {
        self.review_request_with_pin(c, p, None)
    }
    pub fn review_request_with_pin(
        &self,
        c: &str,
        p: ReviewRequestParams,
        pin: Option<ResolvedCodePin>,
    ) -> Answer<ReviewRoundResult> {
        self.r_run(ReviewRequestOp(
            self.identity.office_id.clone(),
            c.into(),
            p,
            pin,
        ))
    }
    pub fn review_list(&self, c: &str, p: ReviewListParams) -> Answer<ReviewListResult> {
        self.r_read(ReviewListRead(c.into(), p))
    }
    pub fn review_inspect(&self, c: &str, p: ReviewInspectParams) -> Answer<ReviewInspectResult> {
        self.review_inspect_sections(c, p, ReviewInspectSections::ALL)
    }
    pub fn review_inspect_sections(
        &self,
        c: &str,
        p: ReviewInspectParams,
        s: ReviewInspectSections,
    ) -> Answer<ReviewInspectResult> {
        self.r_read(ReviewInspectRead(c.into(), p, s))
    }
    pub fn review_events(&self, c: &str, review: &str) -> Answer<Vec<ReviewEventRecord>> {
        self.r_read(ReviewEventsRead(c.into(), review.into()))
    }
    pub fn review_route(&self, c: &str, p: ReviewRouteParams) -> Answer<ReviewRouteResult> {
        self.r_run(ReviewRouteOp(c.into(), p))
    }
    pub fn review_finding(&self, c: &str, p: ReviewFindingParams) -> Answer<ReviewFindingResult> {
        self.r_run(ReviewFindingOpCmd(c.into(), p))
    }
    pub fn review_verdict(&self, c: &str, p: ReviewVerdictParams) -> Answer<ReviewVerdictResult> {
        self.review_verdict_checked(c, p, None)
    }
    pub fn review_verdict_checked(
        &self,
        c: &str,
        p: ReviewVerdictParams,
        rev: Option<u64>,
    ) -> Answer<ReviewVerdictResult> {
        self.r_run(ReviewVerdictOp(c.into(), p, rev))
    }
    pub fn review_resubmit(&self, c: &str, p: ReviewResubmitParams) -> Answer<ReviewRoundResult> {
        self.review_resubmit_with_pin(c, p, None)
    }
    pub fn review_resubmit_with_pin(
        &self,
        c: &str,
        p: ReviewResubmitParams,
        pin: Option<ResolvedCodePin>,
    ) -> Answer<ReviewRoundResult> {
        self.r_run(ReviewResubmitOp(c.into(), p, pin))
    }
    pub fn review_close(&self, c: &str, p: ReviewCloseParams) -> Answer<ReviewMutationResult> {
        self.r_run(ReviewCloseOp(c.into(), p))
    }
}
