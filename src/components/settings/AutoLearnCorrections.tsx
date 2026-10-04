import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface AutoLearnCorrectionsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AutoLearnCorrections: React.FC<AutoLearnCorrectionsProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    return (
      <ToggleSwitch
        checked={getSetting("auto_learn_corrections") ?? true}
        onChange={(enabled) => updateSetting("auto_learn_corrections", enabled)}
        isUpdating={isUpdating("auto_learn_corrections")}
        label={t("settings.learning.autoLearn.label")}
        description={t("settings.learning.autoLearn.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });
