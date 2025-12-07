# Integration Tests

## LLM Inference Test

The `llm_inference_test.rs` demonstrates end-to-end integration of the parking lot scheduler with real OpenAI streaming LLM calls.

### Test Overview

This test validates the core parking lot scheduler functionality:

1. **Resource Pool Configuration**: Creates a pool with `max_units=3` matching 3 tokio worker threads
2. **Task Flooding**: Submits 15 tasks rapidly to exceed capacity and trigger parking behavior
3. **Streaming Support**: Makes real OpenAI API calls with streaming responses
4. **Channel-based Collection**: Uses tokio channels to collect stream chunks from each task
5. **Concurrency Verification**: Verifies that no more than 3 tasks run concurrently

### Requirements

- **Rust 1.90.0** or later
- **OpenAI API Key**: Set the `OPENAI_API_KEY` environment variable
- **Network Access**: The test makes real API calls to OpenAI

### Running the Test

#### Using the helper script:

```bash
export OPENAI_API_KEY=your-api-key-here
./run-llm-test.sh
```

#### Using cargo directly:

```bash
export OPENAI_API_KEY=your-api-key-here
cargo test --features tokio-runtime --test llm_inference_test -- --nocapture
```

### What to Expect

The test will:

1. Submit 15 tasks with simple prompts (e.g., "Count to 3", "Name 2 colors")
2. First 3 tasks start immediately (matching `max_units=3`)
3. Remaining 12 tasks are queued (demonstrating parking behavior)
4. As tasks complete, queued tasks are awakened and executed
5. Stream chunks are collected via channels and logged
6. Peak concurrent task count is verified to not exceed 3

### Test Output

You should see logging output showing:

- Task submission with immediate starts or queuing
- Active task counts as tasks execute
- Peak concurrency verification
- Stream chunk collection
- Final verification that parking behavior worked correctly

### Cost Considerations

This test makes real OpenAI API calls. To minimize costs:

- Prompts are kept very short (e.g., "Count to 3")
- Max tokens is set to 50 per request
- Uses `gpt-3.5-turbo` (lower cost model)
- Typical cost per test run: $0.01-0.02

### Skipping the Test

If `OPENAI_API_KEY` is not set, the test will skip automatically with a message.

### Architecture Demonstration

This test demonstrates several key architectural patterns:

- **Ports & Adapters**: Core scheduler logic is runtime-agnostic
- **Resource Accounting**: Tasks consume capacity units
- **Parking/Queuing**: Tasks wait when capacity is exhausted
- **Mailbox Pattern**: Results delivered asynchronously
- **Stream Handling**: Real-time streaming responses via channels

## Other Tests

- `restart_resume.rs`: Placeholder for persistence/restart testing (currently ignored)
