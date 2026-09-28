//! `next_deadline`: what a host driving the runtime from a real clock needs
//! to know to arm one wall-clock timeout, rather than polling.

use epdf_kernel::rt;

#[test]
fn next_deadline_is_the_earliest_pending_timer() {
    rt::reset();
    rt::spawn(async {
        rt::sleep(30.0).await.unwrap();
    });
    rt::spawn(async {
        rt::sleep(10.0).await.unwrap();
    });
    assert_eq!(rt::next_deadline(), Some(10.0));
    rt::advance(10.0);
    assert_eq!(rt::next_deadline(), Some(30.0));
    rt::advance(20.0);
    assert_eq!(rt::next_deadline(), None);
}
