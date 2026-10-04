import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { TextReplacement } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface TextReplacementsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const TextReplacements: React.FC<TextReplacementsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [from, setFrom] = useState("");
    const [to, setTo] = useState("");
    const replacements: TextReplacement[] =
      getSetting("text_replacements") || [];
    const updating = isUpdating("text_replacements");
    const trimmedFrom = from.trim();

    const handleAdd = () => {
      if (!trimmedFrom || !to.trim()) return;
      if (
        replacements.some(
          (r) => r.from.toLowerCase() === trimmedFrom.toLowerCase(),
        )
      ) {
        toast.error(
          t("settings.learning.replacements.duplicate", { from: trimmedFrom }),
        );
        return;
      }
      updateSetting("text_replacements", [
        ...replacements,
        { from: trimmedFrom, to, learned: false },
      ]);
      setFrom("");
      setTo("");
    };

    const handleRemove = (fromToRemove: string) => {
      updateSetting(
        "text_replacements",
        replacements.filter((r) => r.from !== fromToRemove),
      );
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAdd();
      }
    };

    return (
      <>
        <SettingContainer
          title={t("settings.learning.replacements.title")}
          description={t("settings.learning.replacements.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            <Input
              type="text"
              className="max-w-32"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={t("settings.learning.replacements.fromPlaceholder")}
              variant="compact"
              disabled={updating}
            />
            <Input
              type="text"
              className="max-w-40"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={t("settings.learning.replacements.toPlaceholder")}
              variant="compact"
              disabled={updating}
            />
            <Button
              onClick={handleAdd}
              disabled={!trimmedFrom || !to.trim() || updating}
              variant="primary"
              size="md"
            >
              {t("settings.learning.replacements.add")}
            </Button>
          </div>
        </SettingContainer>
        {replacements.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-wrap gap-1`}
          >
            {replacements.map((r) => (
              <Button
                key={r.from}
                onClick={() => handleRemove(r.from)}
                disabled={updating}
                variant="secondary"
                size="sm"
                className="inline-flex items-center gap-1 cursor-pointer"
                aria-label={t("settings.learning.replacements.remove", {
                  from: r.from,
                })}
              >
                <span>
                  {r.from} → {r.to.replace(/\n/g, "\\n")}
                </span>
                {r.learned && (
                  <span className="text-xs text-logo-primary">
                    {t("settings.learning.replacements.learned")}
                  </span>
                )}
                <svg
                  className="w-3 h-3"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                </svg>
              </Button>
            ))}
          </div>
        )}
      </>
    );
  },
);
