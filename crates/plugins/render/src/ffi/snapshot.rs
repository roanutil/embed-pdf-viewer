//! Snapshot reads retain the existing capture; none starts a render.
use super::*;

/// Capture metadata is available before its image resolves.
#[derive(Clone, uniffi::Record)]
pub struct RenderSnapshot {
    pub page: PageRef,
    pub rect: Rect,
    pub image: Arc<RenderSnapshotImageHandle>,
}

struct SnapshotBound {
    lifetime: AbortSignal,
    image: Promise<Raster>,
}

#[derive(uniffi::Object)]
pub struct RenderSnapshotImageHandle {
    bound: KernelBound<SnapshotBound>,
}

#[uniffi::export]
impl RenderSnapshotImageHandle {
    /// Wait for this capture's pixels. Cancellation ends only this reader's
    /// wait. Clearing the capture or closing its document stops a pending render.
    pub async fn image(&self, cancel: Arc<CancelToken>) -> Result<RenderedImage, ViewerError> {
        self.bound
            .call_promise_cancellable(
                cancel,
                |b| {
                    if b.lifetime.aborted() {
                        return Err(plugin_error(
                            code::INSTANCE_CLOSED,
                            "render",
                            "the snapshot's document is closed",
                            None,
                            None,
                        ));
                    }
                    let (reader, resolve) = Promise::pending();
                    let cancelled = resolve.clone();
                    b.image.on_settle(move |result| resolve.settle(result.clone()));
                    Ok(reader.with_abort(move |_| {
                        cancelled.reject(plugin_error(
                            code::OPERATION_CANCELLED,
                            "render",
                            "snapshot image wait cancelled",
                            None,
                            None,
                        ))
                    }))
                },
                Raster::owned,
            )
            .await
    }
}

fn snapshot(b: &Bound, held: &crate::Snapshot, post: KernelPost) -> RenderSnapshot {
    let pixels = b.pixels.clone();
    let width = held.display_width;
    // Conversion retains the pixel allocation, not the host-table handle.
    let image = held.image.convert(move |image| Raster::read(image, width, &pixels));
    RenderSnapshot {
        page: held.page.into(),
        rect: held.rect.into(),
        image: Arc::new(RenderSnapshotImageHandle {
            bound: KernelBound::new(
                SnapshotBound { lifetime: b.cap.lifetime().clone(), image },
                post,
            ),
        }),
    }
}

#[uniffi::export(with_foreign)]
pub trait RenderSnapshotListener: Send + Sync {
    fn on_snapshot(&self, snapshot: Option<RenderSnapshot>);
    fn on_end(&self);
}

#[uniffi::export]
impl RenderHandle {
    /// Whether capture is permitted (`doc.render` and `doc.content.copy`).
    pub async fn can_snapshot(&self) -> Result<bool, ViewerError> {
        self.bound.call(|b| Ok(open(b)?.can_snapshot())).await
    }

    /// The existing capture, with its pending image; no render is started.
    pub async fn snapshot(&self) -> Result<Option<RenderSnapshot>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| Ok(open(b)?.get_snapshot().as_ref().map(|held| snapshot(b, held, post))))
            .await
    }

    pub async fn clear_snapshot(&self) -> Result<(), ViewerError> {
        self.bound
            .call(|b| {
                open(b)?.clear_snapshot();
                Ok(())
            })
            .await
    }

    /// The capture tool's dragged or held box, in page points.
    pub async fn snapshot_box(&self, page: PageRef) -> Result<Option<Rect>, ViewerError> {
        self.bound.call(move |b| Ok(open(b)?.get_snapshot_box(page.into()).map(Into::into))).await
    }

    /// Captures and clears; ends once on document close, including late subscribers.
    pub fn subscribe_snapshots(
        &self,
        listener: Arc<dyn RenderSnapshotListener>,
    ) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        let post = self.post.clone();
        self.bound.fire(move |b| {
            let ending = listener.clone();
            let bound = Bound { cap: b.cap.clone(), pixels: b.pixels.clone() };
            slot.fill_until_end(
                b.cap.lifetime(),
                || {
                    b.cap.on_snapshot_changed(move |event| {
                        listener.on_snapshot(
                            event
                                .snapshot
                                .as_ref()
                                .map(|held| snapshot(&bound, held, post.clone())),
                        );
                        Ok(())
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
