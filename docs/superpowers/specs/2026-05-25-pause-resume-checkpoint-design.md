# Pause/Resume Translation Jobs Design

## Goal
Add manual `pause` / `resume` controls for long EPUB translation jobs so a job can stop after the current chunk finishes, survive process loss, and continue from persisted checkpoints without re-translating completed work.

## Architecture
The translation worker keeps the current `job_id` for the entire lifecycle and persists two layers of state under the XDG state directory: a JSON job state file for visible status and metrics, and a SQLite checkpoint database for chunk-level recovery. The worker reads job state between chunks, writes completed chunk results to the checkpoint DB as soon as each chunk finishes, and rebuilds the final EPUB from the source archive plus checkpoint data when the run completes.

Pause is cooperative, not signal-based. `pause` marks the job as `Pausing`, the worker finishes the current request/chunk, flushes the checkpoint, then transitions the job to `Paused` and exits cleanly. `resume` reuses the same `job_id`, reopens the checkpoint DB, skips completed chunks, and continues the same translation run.

## User-Facing Behavior
- `pause <job_id>` requests a safe stop for a running job.
- `resume <job_id>` continues a paused or failed job from checkpoints.
- `status <job_id>` shows `Running`, `Pausing`, `Paused`, `Failed`, or `Completed`, plus chunk progress, token counts, retries, current file, and the last error if present.
- `start` continues to create a new job and launch a detached worker.
- `resume` uses the same worker launch path as `start`, but it targets an existing `job_id` and checkpoint set.

Behavior rules:
- `pause` does not interrupt an in-flight API request.
- `pause` takes effect after the current chunk is persisted.
- `resume` is allowed from `Paused` and `Failed`.
- `resume` is rejected for `Completed`.
- A crash, reboot, or network failure must not delete completed chunk work.

## Data Model
### Job state JSON
Extend the existing job state model with:
- `Paused` and `Pausing` job statuses.
- a durable `last_error` string for the most recent failure or pause reason.
- existing metrics for files, chunks, requests, retries, and tokens.

Save the JSON atomically with temp-file + rename so a power loss cannot leave a truncated state file.

### Checkpoint database
Use a per-job SQLite database at `$XDG_STATE_HOME/agent-book-translate/checkpoints/<job_id>.sqlite3`, alongside the JSON state file and logs. The DB stores chunk progress keyed by `chapter_id` and `chunk_index`, including:
- original chunk text
- translated chunk text
- state (`pending`, `processing`, `completed`)
- updated timestamp

The worker must update the checkpoint when a chunk completes, before moving to the next chunk.

### EPUB write path
The final EPUB must be written to a temp file and atomically renamed into place so a crash during packaging does not corrupt the target output.

## Translation Flow
1. `start` or `resume` loads the job state and opens the checkpoint DB.
2. The worker parses the input EPUB and enumerates text chunks in a deterministic order.
3. Before translating each chunk, the worker checks whether that chunk already exists in the checkpoint DB as `completed`.
4. If a completed translation exists, the worker reuses it.
5. If not, the worker translates the chunk, stores the result in the checkpoint DB, updates job metrics, and continues.
6. Between chunks, the worker reloads the job state to detect `Pausing`.
7. If the job is `Pausing`, the worker transitions it to `Paused`, flushes state, and exits successfully after the current chunk boundary.
8. When all chunks are complete, the worker renders the EPUB from source plus checkpoint data and marks the job `Completed`.

## Rendering Constraint
Chunk recovery must not rely on a global string `replace` pass. Translation output must be tied to stable chunk identity so repeated source text does not overwrite the wrong nodes on resume. The parser/engine boundary must expose chunk ordering and identifiers that let the renderer reconstruct each file deterministically from checkpoints.

## Error Handling
- If the network drops mid-job, the current chunk may be retried by the translation client, and completed earlier chunks remain intact in the checkpoint DB.
- If the worker exits unexpectedly, the job remains resumable because completed chunks are already persisted.
- If the final EPUB packaging fails, the job becomes `Failed` but completed checkpoints remain available for `resume`.
- If `pause` is requested while the worker is idle or already paused, the command should be idempotent.

## Testing
- Add unit tests for job status transitions and persisted pause metadata.
- Add checkpoint tests that verify completed chunks survive reload and are skipped on resume.
- Add a render test that proves duplicate source text is handled by chunk identity rather than a naive global replace.
- Add CLI tests for `pause`, `resume`, `status`, and rejection of `resume` on completed jobs.
- Run the existing Rust checks and a real EPUB regression in the Fedora podman container after the implementation lands.
