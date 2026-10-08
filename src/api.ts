// Typed wrappers around the Rust IPC commands.
import { invoke } from "@tauri-apps/api/core";

export type NodeKind = "Begin" | "End" | "Other";

export interface NodeDto {
  index: number | null;
  sfid: string;
  name: string;
  known: boolean;
  kind: NodeKind;
  start: number;
  end: number;
  dataStart: number;
  dataEnd: number;
  children: NodeDto[];
}

export interface ResourceDto {
  name: string | null;
  kind: string;
  nodeIndex: number;
  start: number;
  end: number;
}

export interface SummaryDto {
  pages: number;
  fields: number;
  resources: number;
  byteSize: number;
  problems: number;
}

export interface ProblemDto {
  message: string;
  at: number;
}

export interface DocumentDto {
  docId: string;
  fileName: string;
  root: NodeDto;
  summary: SummaryDto;
  resources: ResourceDto[];
  problems: ProblemDto[];
}

export interface ImageDto {
  format: "jpeg" | "unsupported";
  base64: string;
}

export interface TextDto {
  x: number;
  y: number;
  text: string;
}

export interface PageLayoutDto {
  pageCount: number;
  widthLu: number;
  heightLu: number;
  unitsPerInch: number;
  texts: TextDto[];
}

export function openAfp(path: string): Promise<DocumentDto> {
  return invoke<DocumentDto>("open_afp", { path });
}

export function getHexSlice(
  docId: string,
  start: number,
  len: number,
): Promise<string> {
  return invoke<string>("get_hex_slice", { docId, start, len });
}

export function getResourceBytes(
  docId: string,
  nodeIndex: number,
): Promise<ImageDto> {
  return invoke<ImageDto>("get_resource_bytes", { docId, nodeIndex });
}

export function getPageLayout(
  docId: string,
  pageIndex: number,
): Promise<PageLayoutDto> {
  return invoke<PageLayoutDto>("get_page_layout", { docId, pageIndex });
}
