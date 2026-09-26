using SearchKit.Web;

namespace SearchKit.Tests;

public class ToolCallsTests
{
    [Theory]
    [InlineData("read_hardening_state")]
    [InlineData("apply_hardening_tweak")]
    [InlineData("revert_hardening_tweak")]
    [InlineData("revert_all_hardening")]
    [InlineData("export_hardening_backup")]
    [InlineData("screenrec_start")]
    [InlineData("screenrec_stop")]
    [InlineData("screenrec_status")]
    [InlineData("screenrec_pause")]
    [InlineData("screenrec_resume")]
    [InlineData("screenrec_export_gif")]
    [InlineData("screenrec_open_region_selector")]
    [InlineData("screenrec_close_region_selector")]
    [InlineData("screenrec_open_redact_selector")]
    [InlineData("screenrec_close_redact_selector")]
    [InlineData("screenrec_list_windows")]
    [InlineData("screenrec_open_toolbar")]
    [InlineData("screenrec_close_toolbar")]
    [InlineData("create_reminder_task")]
    [InlineData("delete_reminder_task")]
    [InlineData("reconcile_reminder_tasks")]
    public void Unavailable_tool_commands_are_refused(string command) =>
        Assert.False(string.IsNullOrWhiteSpace(ToolCalls.Refused(command)));

    [Theory]
    [InlineData("analyze_system_cleaner")]
    [InlineData("preview_duplicate_file")]
    [InlineData("ffmpeg_status")]
    [InlineData("get_clipboard_history")]
    [InlineData("host:open.file")]
    [InlineData("host:opener.reveal")]
    public void Working_tools_remain_available(string command) =>
        Assert.Null(ToolCalls.Refused(command));
}
