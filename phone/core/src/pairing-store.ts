/**
 * What pairing leaves on the phone. The keys go to the `SecretStore` (the system keystore);
 * everything else goes to the `KeyValueStore`. Pairing happens once: this record is what makes
 * every later start connect without asking anything.
 */

import { bytesToHex, equalBytes, hexToBytes } from "./bytes.ts";
import { isJsonObject, numberField, stringField } from "./json.ts";
import { KEY_LENGTH, publicKeyOf } from "./noise.ts";
import type { KeyValueStore, Log, SecretStore } from "./platform.ts";

/** A pairing with one gateway. */
export interface Pairing {
  /** This device's id, given by the gateway. */
  readonly deviceId: string;
  readonly deviceName: string;
  readonly platform: string;
  /** The Mac's name, as it last stated it. */
  readonly gatewayName: string;
  /** `full` or `watch`, as the gateway last stated it. */
  readonly scope: string;
  readonly gatewayPublicKey: Uint8Array;
  readonly devicePrivateKey: Uint8Array;
  readonly devicePublicKey: Uint8Array;
  /** The gateway's port from the pairing code. */
  readonly port: number;
  /** The addresses from the pairing code. */
  readonly addresses: readonly string[];
  readonly pairedAt: number;
}

const VERSION = 1;

export class PairingStore {
  private readonly store: KeyValueStore;
  private readonly secrets: SecretStore;
  private readonly recordKey: string;
  private readonly keysKey: string;
  private readonly log: Log | undefined;

  constructor(store: KeyValueStore, secrets: SecretStore, namespace: string, log?: Log) {
    this.store = store;
    this.secrets = secrets;
    this.recordKey = `${namespace}pairing`;
    this.keysKey = `${namespace}keys`;
    this.log = log;
  }

  /** The stored pairing, or null when there is none or it cannot be used. */
  async load(): Promise<Pairing | null> {
    const [record, keys] = await Promise.all([this.store.get(this.recordKey), this.secrets.get(this.keysKey)]);
    if (record === null || keys === null) {
      if (record !== null) this.log?.("the pairing is stored without its keys; the app is not paired");
      return null;
    }
    try {
      const r: unknown = JSON.parse(record);
      const k: unknown = JSON.parse(keys);
      if (!isJsonObject(r) || !isJsonObject(k) || r["v"] !== VERSION || k["v"] !== VERSION) return null;
      const devicePrivateKey = hexToBytes(stringField(k, "devicePrivateKey") ?? "");
      const devicePublicKey = hexToBytes(stringField(k, "devicePublicKey") ?? "");
      const gatewayPublicKey = hexToBytes(stringField(k, "gatewayPublicKey") ?? "");
      const deviceId = stringField(r, "deviceId");
      const port = numberField(r, "port");
      const addresses = r["addresses"];
      if (devicePrivateKey.length !== KEY_LENGTH || gatewayPublicKey.length !== KEY_LENGTH) return null;
      if (!equalBytes(publicKeyOf(devicePrivateKey), devicePublicKey)) return null;
      if (!deviceId || port === null || !Array.isArray(addresses)) return null;
      return {
        deviceId,
        deviceName: stringField(r, "deviceName") ?? "",
        platform: stringField(r, "platform") ?? "",
        gatewayName: stringField(r, "gatewayName") ?? "",
        scope: stringField(r, "scope") ?? "",
        gatewayPublicKey,
        devicePrivateKey,
        devicePublicKey,
        port,
        addresses: addresses.filter((a): a is string => typeof a === "string"),
        pairedAt: numberField(r, "pairedAt") ?? 0,
      };
    } catch {
      this.log?.("the stored pairing cannot be read; the app is not paired");
      return null;
    }
  }

  /** Stores a pairing: the keys first, so a record never exists without them. */
  async save(pairing: Pairing): Promise<void> {
    await this.secrets.set(
      this.keysKey,
      JSON.stringify({
        v: VERSION,
        devicePrivateKey: bytesToHex(pairing.devicePrivateKey),
        devicePublicKey: bytesToHex(pairing.devicePublicKey),
        gatewayPublicKey: bytesToHex(pairing.gatewayPublicKey),
      }),
    );
    await this.saveRecord(pairing);
  }

  /** Stores what may change after pairing: the names and the scope. */
  async saveRecord(pairing: Pairing): Promise<void> {
    await this.store.set(
      this.recordKey,
      JSON.stringify({
        v: VERSION,
        deviceId: pairing.deviceId,
        deviceName: pairing.deviceName,
        platform: pairing.platform,
        gatewayName: pairing.gatewayName,
        scope: pairing.scope,
        port: pairing.port,
        addresses: pairing.addresses,
        pairedAt: pairing.pairedAt,
      }),
    );
  }

  /** Deletes the pairing: the record first, then the keys. */
  async forget(): Promise<void> {
    await this.store.delete(this.recordKey);
    await this.secrets.delete(this.keysKey);
  }
}
