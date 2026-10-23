// Kotlin smoke test for the generated bindings (cr-44). Compiled with the generated
// parking_lot_ffi.kt by bindings/smoke/kotlin.sh and run on a host JVM against the
// host build of the library (JNA loads libparking_lot_ffi from jna.library.path).
// Exercises the suspend surface end to end, including a close and reopen.
import kotlinx.coroutines.runBlocking
import uniffi.parking_lot_ffi.DeliveryResult
import uniffi.parking_lot_ffi.FfiException
import uniffi.parking_lot_ffi.MailboxAddress
import uniffi.parking_lot_ffi.NewTask
import uniffi.parking_lot_ffi.ResourceAmount
import uniffi.parking_lot_ffi.ResourceClass
import uniffi.parking_lot_ffi.ResultDelivery
import uniffi.parking_lot_ffi.StoreConfig
import uniffi.parking_lot_ffi.TaskOutcome
import uniffi.parking_lot_ffi.TaskPriority
import uniffi.parking_lot_ffi.openStore
import java.nio.file.Files
import kotlin.system.exitProcess

private fun check(condition: Boolean, message: String) {
    if (!condition) {
        System.err.println("FAIL: $message")
        exitProcess(1)
    }
}

fun main() = runBlocking {
    val dir = Files.createTempDirectory("parking-lot-ffi-smoke")
    val config = StoreConfig(dir.resolve("app.redb").toString(), 100UL)
    val mailbox = MailboxAddress("app", "u1", null)
    val payload = "kotlin-payload".toByteArray()

    // First run: enqueue, then close the store (app closes).
    val first = openStore(config)
    val accepted = first.enqueue(
        NewTask(
            7UL, payload, TaskPriority.HIGH,
            listOf(ResourceAmount(ResourceClass.CPU, 1u)),
            "req-7", mailbox, null,
        ),
    )
    check(!accepted.duplicate, "first enqueue is not a duplicate")
    first.close()

    // Second run: the task is still there and leasable.
    val second = openStore(config)
    val leased = second.lease(5u, 60_000UL)
    check(leased.size == 1, "one task after reopen, got ${leased.size}")
    check(leased[0].taskId == 7UL, "task id survives")
    check(leased[0].payload.contentEquals(payload), "payload survives")

    // Deliver the result, then ack: the order the contract requires.
    val delivered = second.deliverResult(
        ResultDelivery(mailbox, 7UL, TaskOutcome.Completed, "done".toByteArray(), "req-7", null),
    )
    check(delivered == DeliveryResult.STORED, "result stored")
    second.ack(leased[0].lease)

    // A stale handle is a typed error.
    try {
        second.ack(leased[0].lease)
        check(false, "second ack must fail")
    } catch (e: FfiException.LeaseLost) {
        // expected
    }

    val entry = second.consumeResult(mailbox)
    check(entry?.payload?.contentEquals("done".toByteArray()) == true, "result collected")
    check(second.consumeResult(mailbox) == null, "result is collected once")
    second.close()
    println("kotlin smoke: ok")
}
