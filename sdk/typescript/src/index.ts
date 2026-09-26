/**
 * TypeScript SDK for the anastasia harness API.
 *
 * ```ts
 * import { AnastasiaClient } from "@1jehuang/anastasia-sdk";
 * const client = await AnastasiaClient.connect({ clientName: "my-app/1.0" });
 * const session = await client.createSession(process.cwd());
 * const turn = await client.run(session.session_id, "hello");
 * console.log(turn.text);
 * client.close();
 * ```
 */

export * from "./protocol.js";
export * from "./sockets.js";
export * from "./framing.js";
export { HarnessError } from "./errors.js";
export {
  launchInstance,
  inheritCredentials,
  userAnastasiaHome,
  userAppConfigDir,
} from "./launch.js";
export type { LaunchOptions, LaunchedInstance } from "./launch.js";
export { bundledAnastasiaBinary, platformBinaryPackage } from "./binary.js";
export { AnastasiaClient, unixSocketTransport } from "./client.js";
export type {
  ConnectOptions,
  FileContent,
  FileStatus,
  GlobalEventsOptions,
  RunOptions,
  RunStructuredOptions,
  RuntimeInfo,
  SendMessageOptions,
  StructuredTurnResult,
  Transport,
  TurnResult,
} from "./client.js";
export { StructuredOutputError } from "./structured.js";
export type {
  StructuredOutputAttempt,
  StructuredOutputSchema,
  StructuredValidationIssue,
} from "./structured.js";
