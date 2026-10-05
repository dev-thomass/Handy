import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands, type AutoLearnLogEntry } from "@/bindings";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface AutoLearnLogProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** What auto-learn saw after each dictation, and why it learned or not. */
export const AutoLearnLog: React.FC<AutoLearnLogProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t, i18n } = useTranslation();
    const [open, setOpen] = useState(false);
    const [entries, setEntries] = useState<AutoLearnLogEntry[]>([]);

    const refresh = useCallback(async () => {
      setEntries(await commands.getAutoLearnLog());
    }, []);

    useEffect(() => {
      if (!open) return;
      refresh();
      const timer = setInterval(refresh, 3000);
      return () => clearInterval(timer);
    }, [open, refresh]);

    const describe = (entry: AutoLearnLogEntry) =>
      t(`settings.learning.autoLearnLog.codes.${entry.code}`, {
        misheard: entry.misheard ?? "",
        word: entry.word ?? "",
        defaultValue: entry.code,
      });

    const time = (entry: AutoLearnLogEntry) =>
      new Date(entry.at).toLocaleTimeString(i18n.language, {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });

    const handleCopy = async () => {
      const text = entries
        .map((e) => `${time(e)}  ${e.app ?? "?"}  ${e.code}  ${describe(e)}`)
        .join("\n");
      await navigator.clipboard.writeText(text);
      toast.success(t("settings.learning.autoLearnLog.copied"));
    };

    const handleClear = async () => {
      await commands.clearAutoLearnLog();
      setEntries([]);
    };

    return (
      <>
        <SettingContainer
          title={t("settings.learning.autoLearnLog.title")}
          description={t("settings.learning.autoLearnLog.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            {open && entries.length > 0 && (
              <>
                <Button onClick={handleCopy} variant="secondary" size="sm">
                  {t("settings.learning.autoLearnLog.copy")}
                </Button>
                <Button onClick={handleClear} variant="secondary" size="sm">
                  {t("settings.learning.autoLearnLog.clear")}
                </Button>
              </>
            )}
            <Button
              onClick={() => setOpen((o) => !o)}
              variant="secondary"
              size="sm"
            >
              {open
                ? t("settings.learning.autoLearnLog.hide")
                : t("settings.learning.autoLearnLog.show")}
            </Button>
          </div>
        </SettingContainer>
        {open && (
          <div
            className={`px-4 py-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} max-h-64 overflow-y-auto text-xs`}
          >
            {entries.length === 0 ? (
              <p className="text-mid-gray">
                {t("settings.learning.autoLearnLog.empty")}
              </p>
            ) : (
              <ul className="space-y-1">
                {entries.map((entry, i) => (
                  <li key={`${entry.at}-${i}`} className="flex gap-2">
                    <span className="text-mid-gray tabular-nums shrink-0">
                      {time(entry)}
                    </span>
                    {entry.app && (
                      <span className="text-mid-gray shrink-0">
                        {entry.app}
                      </span>
                    )}
                    <span
                      className={
                        entry.code === "learned" ? "text-logo-primary" : ""
                      }
                    >
                      {describe(entry)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </>
    );
  },
);
