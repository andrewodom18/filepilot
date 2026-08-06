import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { StorageItemCard } from "./App";
import type { StorageItem } from "./types";

const item: StorageItem = {
  label: "User Library / Caches",
  path: "/Users/test/Library/Caches/Example.app",
  sizeBytes: 1024,
  sizeKnown: true,
  isDirectory: true,
  cleanupAllowed: true,
  assessment: "likely-safe-to-review",
  reason: "Cache data that may be recreated.",
  recommendation: "Review the owning app first.",
  children: [],
};

describe("StorageItemCard", () => {
  it("exposes inspection and constrained cleanup actions", () => {
    const onAnalyze = vi.fn();
    const onCleanup = vi.fn().mockResolvedValue(undefined);
    render(<StorageItemCard item={item} onAnalyze={onAnalyze} onCleanup={onCleanup} />);

    expect(screen.getByText("User Library / Caches")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
    fireEvent.click(screen.getByRole("button", { name: "Move to Trash" }));

    expect(onAnalyze).toHaveBeenCalledWith(item.path);
    expect(onCleanup).toHaveBeenCalledWith(item);
  });
});
