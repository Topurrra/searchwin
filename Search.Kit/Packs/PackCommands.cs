using SearchKit.Commands;

namespace SearchKit.Packs;

/// The pack actions shared by the field and any other command caller.
public static class PackCommands
{
    public static void Register(CommandRegistry registry, Action show, Action installFfmpeg)
    {
        registry.Add(new Command("open.ffmpeg", "FFmpeg Pack", Tier.Read, ["ffmpeg"], Group: "Packs"), _ => show());
        registry.Add(new Command("packs.ffmpeg", "Install FFmpeg", Tier.Act, ["install ffmpeg"], Group: "Packs"), _ =>
        {
            show();
            installFfmpeg();
        });
    }
}
