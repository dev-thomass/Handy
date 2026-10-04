import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface AutoPostProcessProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AutoPostProcess: React.FC<AutoPostProcessProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    return (
      <ToggleSwitch
        checked={getSetting("auto_post_process") || false}
        onChange={(enabled) => updateSetting("auto_post_process", enabled)}
        isUpdating={isUpdating("auto_post_process")}
        label={t("settings.learning.autoPostProcess.label")}
        description={t("settings.learning.autoPostProcess.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
