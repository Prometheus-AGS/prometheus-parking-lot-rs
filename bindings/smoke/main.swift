// Swift smoke test for the generated bindings (cr-44). Compiled together with the
// generated parking_lot_ffi.swift and linked against libparking_lot_ffi by
// bindings/smoke/swift.sh. Exercises the async surface end to end, including a
// close-and-reopen of the store file.
import Foundation

func check(_ condition: Bool, _ message: String) {
    if !condition {
        FileHandle.standardError.write(Data("FAIL: \(message)\n".utf8))
        exit(1)
    }
}

let dir = FileManager.default.temporaryDirectory
    .appendingPathComponent("parking-lot-ffi-smoke-\(UUID().uuidString)")
let path = dir.appendingPathComponent("app.redb").path
let config = StoreConfig(path: path, maxDepth: 100)
let mailbox = MailboxAddress(tenant: "app", userId: "u1", sessionId: nil)

do {
    // First run: enqueue, then drop the store (app closes).
    var store: ParkingLotStore? = try await openStore(config: config)
    let accepted = try await store!.enqueue(task: NewTask(
        taskId: 7,
        payload: Data("swift-payload".utf8),
        priority: .high,
        cost: [ResourceAmount(resource: .cpu, units: 1)],
        idempotencyKey: "req-7",
        mailbox: mailbox,
        deadlineMs: nil))
    check(!accepted.duplicate, "first enqueue is not a duplicate")
    store = nil

    // Second run: the task is still there and leasable.
    store = try await openStore(config: config)
    let leased = try await store!.lease(max: 5, leaseMs: 60_000)
    check(leased.count == 1, "one task after reopen, got \(leased.count)")
    check(leased[0].taskId == 7, "task id survives")
    check(leased[0].payload == Data("swift-payload".utf8), "payload survives")

    // Deliver the result, then ack: the order the contract requires.
    let delivered = try await store!.deliverResult(delivery: ResultDelivery(
        mailbox: mailbox, taskId: 7, outcome: .completed,
        payload: Data("done".utf8), idempotencyKey: "req-7", ttlMs: nil))
    check(delivered == .stored, "result stored")
    try await store!.ack(lease: leased[0].lease)

    // A stale handle is a typed error.
    do {
        try await store!.ack(lease: leased[0].lease)
        check(false, "second ack must fail")
    } catch FfiError.LeaseLost {
        // expected
    }

    let entry = try await store!.consumeResult(mailbox: mailbox)
    check(entry?.payload == Data("done".utf8), "result collected")
    let again = try await store!.consumeResult(mailbox: mailbox)
    check(again == nil, "result is collected once")
    print("swift smoke: ok")
} catch {
    FileHandle.standardError.write(Data("FAIL: unexpected error \(error)\n".utf8))
    exit(1)
}
