import { portByCode } from "@harbor/dispatch/ports/registry";

function stamp(instant: Date, timeZone: string): string {
  const parts = new Intl.DateTimeFormat("en-GB", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(instant);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return `${get("year")}-${get("month")}-${get("day")} ${get("hour")}:${get("minute")}`;
}

export function localArrival(eta: Date, locode: string): string {
  const port = portByCode(locode);
  if (!port) {
    throw new RangeError(`unknown port: ${locode}`);
  }
  return stamp(eta, "UTC");
}
