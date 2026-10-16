import type { Booking } from "@harbor/dispatch/types";

function overlaps(a: Booking, b: Booking): boolean {
  return a.start.getTime() <= b.end.getTime() && b.start.getTime() <= a.end.getTime();
}

export class BerthSchedule {
  #bookings: Booking[] = [];

  book(booking: Booking): void {
    if (booking.end.getTime() <= booking.start.getTime()) {
      throw new RangeError("a booking must end after it starts");
    }
    const clash = this.#bookings.find((b) => b.berth === booking.berth && overlaps(b, booking));
    if (clash) {
      throw new Error(`berth ${booking.berth} is already booked for ${clash.vessel}`);
    }
    this.#bookings.push(booking);
  }

  bookingsFor(berth: string): Booking[] {
    return this.#bookings
      .filter((b) => b.berth === berth)
      .sort((a, b) => a.start.getTime() - b.start.getTime());
  }
}
