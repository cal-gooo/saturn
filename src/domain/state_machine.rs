use crate::{
    domain::entities::OrderState,
    errors::{ApiError, AppResult},
};

pub fn ensure_transition(current: OrderState, next: OrderState) -> AppResult<()> {
    let allowed = match current {
        OrderState::Created => matches!(next, OrderState::Quoted),
        OrderState::Quoted => matches!(
            next,
            OrderState::PaymentPending | OrderState::Expired | OrderState::Cancelled
        ),
        OrderState::PaymentPending => matches!(
            next,
            OrderState::Paid | OrderState::Expired | OrderState::Disputed
        ),
        OrderState::Paid => matches!(next, OrderState::Fulfilled | OrderState::Disputed),
        OrderState::Disputed => matches!(next, OrderState::Paid),
        OrderState::Expired | OrderState::Cancelled | OrderState::Fulfilled => false,
    };

    if allowed {
        Ok(())
    } else {
        Err(ApiError::state_transition_invalid(format!(
            "cannot transition from {current} to {next}"
        )))
    }
}
