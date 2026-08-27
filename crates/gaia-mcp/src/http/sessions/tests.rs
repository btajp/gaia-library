use super::*;

#[tokio::test]
async fn default_capacity_is_bounded_and_closing_releases_every_slot() {
    let sessions = OwnedSessionManager::default();
    let mut transports = Vec::new();
    for _ in 0..MAX_SESSIONS {
        let (id, transport) = sessions.create_session().await.unwrap();
        assert!(!sessions.is_owned_by(&id, "me").await.unwrap());
        transports.push(transport);
    }
    assert!(matches!(
        sessions.create_session().await,
        Err(SessionError::Capacity)
    ));
    assert_eq!(sessions.inner.sessions.read().await.len(), MAX_SESSIONS);
    sessions.close_all().await.unwrap();
    assert_eq!(sessions.available_slots(), MAX_SESSIONS);
    assert!(sessions.owners.lock().unwrap().is_empty());
    assert!(sessions.inner.sessions.read().await.is_empty());
    drop(transports);
}

#[tokio::test]
async fn owner_cannot_be_rebound_to_another_client() {
    let sessions = OwnedSessionManager::default();
    let (id, _transport) = sessions.create_session().await.unwrap();
    sessions.bind_owner(&id, "me".into()).unwrap();
    assert!(matches!(
        sessions.bind_owner(&id, "bot".into()),
        Err(SessionError::OwnerMismatch)
    ));
    assert!(sessions.is_owned_by(&id, "me").await.unwrap());
    assert!(!sessions.is_owned_by(&id, "bot").await.unwrap());
    sessions.close_all().await.unwrap();
}
