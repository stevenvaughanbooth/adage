export interface Port {
  code: string;
  name: string;
  country: string;
  timezone: string;
  maxDraftM: number;
  pilotageHours: number;
}

export interface VesselClass {
  name: string;
  lengthM: number;
  beamM: number;
  draftM: number;
  teu: number;
}

export interface Berth {
  id: string;
  port: string;
  lengthM: number;
  depthM: number;
}

export type ContainerLength = 20 | 40 | 45;

export interface Container {
  id: string;
  lengthFt: ContainerLength;
  cargoKg: number;
  tareKg: number;
  reefer?: boolean;
  dgClass?: string;
}

export interface Booking {
  berth: string;
  vessel: string;
  start: Date;
  end: Date;
}
