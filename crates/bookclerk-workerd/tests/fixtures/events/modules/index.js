/**
 * Event contract fixture: `event(batch)` outcome variants recorded on the
 * delivered `EventMessage` (ack / retry / reject / deadLetter / suspend).
 */

import { BookclerkEntrypoint } from "@bookclerk/plugin-sdk/workerd";

export default class EventPlugin extends BookclerkEntrypoint {
  async describe() {
    return { displayName: "Event contract fixture" };
  }

  async event(batch) {
    for (const msg of batch.messages) {
      switch (msg.type) {
        case "test_retry":
          msg.retry({ retryAt: 1, reason: "echo retry" });
          break;
        case "test_reject":
          msg.reject("echo reject");
          break;
        case "test_dead_letter":
          msg.deadLetter("echo dead letter");
          break;
        case "test_suspend":
          msg.suspend({ checkpoint: { n: 1 }, checkpointSchemaVersion: 1, wakeAt: 1 });
          break;
        default:
          msg.ack();
      }
    }
  }
}
