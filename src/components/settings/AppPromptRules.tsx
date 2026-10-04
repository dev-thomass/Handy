import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type { AppPromptRule } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface AppPromptRulesProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AppPromptRules: React.FC<AppPromptRulesProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [appMatch, setAppMatch] = useState("");
    const [promptId, setPromptId] = useState<string | null>(null);
    const rules: AppPromptRule[] = getSetting("app_prompt_rules") || [];
    const prompts = getSetting("post_process_prompts") || [];
    const updating = isUpdating("app_prompt_rules");
    const trimmedApp = appMatch.trim();

    const promptName = (id: string) =>
      prompts.find((p) => p.id === id)?.name ?? id;

    const handleAdd = () => {
      if (!trimmedApp || !promptId) return;
      updateSetting("app_prompt_rules", [
        ...rules.filter(
          (r) => r.app_match.toLowerCase() !== trimmedApp.toLowerCase(),
        ),
        { app_match: trimmedApp, prompt_id: promptId },
      ]);
      setAppMatch("");
    };

    const handleRemove = (index: number) => {
      updateSetting(
        "app_prompt_rules",
        rules.filter((_, i) => i !== index),
      );
    };

    return (
      <>
        <SettingContainer
          title={t("settings.learning.appPrompts.title")}
          description={t("settings.learning.appPrompts.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            <Input
              type="text"
              className="max-w-32"
              value={appMatch}
              onChange={(e) => setAppMatch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  handleAdd();
                }
              }}
              placeholder={t("settings.learning.appPrompts.appPlaceholder")}
              variant="compact"
              disabled={updating}
            />
            <Dropdown
              className="min-w-36"
              selectedValue={promptId}
              options={prompts.map((p) => ({ value: p.id, label: p.name }))}
              onSelect={setPromptId}
              placeholder={t("settings.learning.appPrompts.promptPlaceholder")}
              disabled={updating || prompts.length === 0}
            />
            <Button
              onClick={handleAdd}
              disabled={!trimmedApp || !promptId || updating}
              variant="primary"
              size="md"
            >
              {t("settings.learning.appPrompts.add")}
            </Button>
          </div>
        </SettingContainer>
        {rules.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-col gap-1`}
          >
            <div className="flex flex-wrap gap-1">
              {rules.map((rule, index) => (
                <Button
                  key={`${rule.app_match}-${index}`}
                  onClick={() => handleRemove(index)}
                  disabled={updating}
                  variant="secondary"
                  size="sm"
                  className="inline-flex items-center gap-1 cursor-pointer"
                  aria-label={t("settings.learning.appPrompts.remove", {
                    app: rule.app_match,
                  })}
                >
                  <span>
                    {rule.app_match} → {promptName(rule.prompt_id)}
                  </span>
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
            <p className="text-xs text-mid-gray/70">
              {t("settings.learning.appPrompts.fallback")}
            </p>
          </div>
        )}
      </>
    );
  },
);
