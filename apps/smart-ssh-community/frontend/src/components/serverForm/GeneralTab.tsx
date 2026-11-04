import { useTranslation } from "react-i18next";
import { flattenGroupOptions, groupOptionLabel } from "../../groupTree";
import { TECHNICAL_INPUT_PROPS } from "../../technicalInputProps";
import type { GroupDto } from "../../types";
import { TabPanel } from "../ui";
import { Field, FIELD_CONTROL_CLASS } from "../ui/Field";
import type { ServerModel } from "./formTypes";
import { TagsField } from "./TagsField";

interface Props {
  idBase: string;
  active: boolean;
  model: ServerModel;
  allGroups: GroupDto[];
}

/** General tab of a remote server: name, group and tags. */
export function GeneralTab({ idBase, active, model, allGroups }: Props) {
  const { t } = useTranslation();
  const { name, setName, groupId, setGroupId, tags, setTags } = model;
  return (
    <TabPanel idBase={idBase} id="general" active={active} className="space-y-3">
          <Field label={t("common.name")}>
            {(control) => (
              <input
                {...control}
                {...TECHNICAL_INPUT_PROPS}
                type="text"
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                className={`${FIELD_CONTROL_CLASS} w-full`}
              />
            )}
          </Field>

        <Field label={t("serverForm.group")}>
          {(control) => (
            <select
              {...control}
              value={groupId ?? ""}
              onChange={(e) => setGroupId(e.target.value || null)}
              className={`${FIELD_CONTROL_CLASS} w-full`}
            >
              <option value="">{t("serverForm.noGroup")}</option>
              {/* Issue #49: hierarchisch (eingerückter voller Pfad), damit
               * eine vorbelegte Untergruppe erkennbar ist. */}
              {flattenGroupOptions(allGroups).map((option) => (
                <option key={option.group.id} value={option.group.id}>
                  {groupOptionLabel(option)}
                </option>
              ))}
            </select>
          )}
        </Field>
      <TagsField tags={tags} setTags={setTags} />
    </TabPanel>
  );
}
