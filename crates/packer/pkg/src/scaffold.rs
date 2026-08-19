//! 会话根脚手架：三个 agent skills（loomgui-editor 围栏规则手册 / loomgui-runtime
//! 运行时接线手册 / loom CLI 手册），从模板覆盖拷入。CLI `loom init` / `loom scaffold`
//! 与 GUI「新建工作区」共用（模板只此一份，防两端口径漂移）。模板文件在 crate 的
//! templates/ 下（include_str! 嵌进二进制——CLI 单 exe 分发）。

use std::path::Path;

/// 单个 skill 的落位产物：目录名 + (相对路径, 内容) 清单。
struct SkillArtifacts {
    dir: &'static str,
    files: Vec<(&'static str, &'static str)>,
}

/// editor skill 的深度参考表（渐进披露：SKILL.md 是操作手册，查找表按需加载）。
/// 元组首项是相对 skill 目录的路径（含 references/ 前缀）。
const EDITOR_REFERENCES: &[(&str, &str)] = &[
    (
        "references/fence-schema.md",
        include_str!("../templates/editor/references/fence-schema.md"),
    ),
    (
        "references/css-reference.md",
        include_str!("../templates/editor/references/css-reference.md"),
    ),
    (
        "references/patterns.md",
        include_str!("../templates/editor/references/patterns.md"),
    ),
];

/// runtime skill 的深度参考表（渐进披露：SKILL.md 是接线手册，API 查找表按需
/// 加载）。内容镜像随包 C# 签名契约——消费者 agent 查 API 不需要 LoomGUI 源码仓库。
const RUNTIME_REFERENCES: &[(&str, &str)] = &[(
    "references/api-reference.md",
    include_str!("../templates/runtime/references/api-reference.md"),
)];

fn skill_artifacts() -> Vec<SkillArtifacts> {
    vec![
        SkillArtifacts {
            dir: "loomgui-editor",
            files: vec![("SKILL.md", include_str!("../templates/editor/SKILL.md"))],
        },
        SkillArtifacts {
            dir: "loomgui-runtime",
            files: vec![("SKILL.md", include_str!("../templates/runtime/SKILL.md"))],
        },
        SkillArtifacts {
            dir: "loom",
            files: vec![("SKILL.md", include_str!("../templates/loom/SKILL.md"))],
        },
    ]
}

/// 按 agent 类型写 skills 到会话根：`claude` 落 `.claude/skills/`，`agents` 落
/// `.agents/skills/`（AGENTS.md 约定的通用目录）。覆盖拷入（模板升级后重跑
/// init / scaffold 即刷新），不生成任何指令文档——skill 描述句负责触发，
/// 内容三件全在 skill 目录内。不碰 workspace.json 和源文件。
pub fn write_agent_scaffold(root: &Path, agents: &[String]) -> Result<(), String> {
    if agents.is_empty() {
        return Err("no agent kind selected (expected `claude` and/or `agents`)".to_string());
    }
    let mut skills = skill_artifacts();
    // references/ 并入对应 skill 的产物清单（editor 三件 + runtime API 查找表）。
    skills[0].files.extend_from_slice(EDITOR_REFERENCES);
    skills[1].files.extend_from_slice(RUNTIME_REFERENCES);

    for agent in agents {
        let skills_dir = match agent.as_str() {
            "claude" => ".claude/skills",
            "agents" => ".agents/skills",
            other => return Err(format!("unknown agent kind: {other}")),
        };
        for skill in &skills {
            let dir = root.join(skills_dir).join(skill.dir);
            std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
            for (rel, content) in &skill.files {
                if let Some(parent) = dir.join(rel).parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("create {}: {e}", parent.display()))?;
                }
                std::fs::write(dir.join(rel), content)
                    .map_err(|e| format!("write {}: {e}", dir.join(rel).display()))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 脚手架产物契约：三 skill + editor references 落位；无指令文档；未知 kind 拒绝。
    #[test]
    fn scaffold_writes_three_skills_per_agent() {
        let tmp = std::env::temp_dir().join(format!("loom_scaffold_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        write_agent_scaffold(&tmp, &["claude".to_string(), "agents".to_string()]).unwrap();

        for skills in [".claude/skills", ".agents/skills"] {
            assert!(
                tmp.join(skills).join("loomgui-editor/SKILL.md").is_file(),
                "{skills} 的 editor skill 须落位"
            );
            assert!(
                tmp.join(skills)
                    .join("loomgui-editor/references/fence-schema.md")
                    .is_file(),
                "{skills} 的 editor references 须落位"
            );
            assert!(
                tmp.join(skills).join("loomgui-runtime/SKILL.md").is_file(),
                "{skills} 的 runtime skill 须落位"
            );
            assert!(
                tmp.join(skills)
                    .join("loomgui-runtime/references/api-reference.md")
                    .is_file(),
                "{skills} 的 runtime references 须落位"
            );
            let loom = std::fs::read_to_string(tmp.join(skills).join("loom/SKILL.md")).unwrap();
            assert!(loom.contains("loom check"), "loom skill 须含命令面");
        }
        // 不再生成指令文档（AGENTS.md / CLAUDE.md 由用户自持）。
        assert!(!tmp.join("AGENTS.md").exists());
        assert!(!tmp.join("CLAUDE.md").exists());
        // 未知 agent kind 拒绝。
        assert!(write_agent_scaffold(&tmp, &["vscode".to_string()]).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
