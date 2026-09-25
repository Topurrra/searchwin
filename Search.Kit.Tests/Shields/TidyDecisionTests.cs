using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

public class TidyDecisionTests
{
    [Fact]
    public void AnOrdinaryGetCanBeTidied() =>
        Assert.True(TidyDecision.CanTidy(hasRequestBody: false, redirectStatus: 0));

    [Fact]
    public void AFormPostIsNeverTidied() =>
        Assert.False(TidyDecision.CanTidy(hasRequestBody: true, redirectStatus: 0));

    [Fact]
    public void A307RedirectIsNeverTidied() =>
        Assert.False(TidyDecision.CanTidy(hasRequestBody: false, redirectStatus: 307));

    [Fact]
    public void A308RedirectIsNeverTidied() =>
        Assert.False(TidyDecision.CanTidy(hasRequestBody: false, redirectStatus: 308));

    [Fact]
    public void AnOrdinaryRedirectCanStillBeTidied() =>
        Assert.True(TidyDecision.CanTidy(hasRequestBody: false, redirectStatus: 301));

    [Fact]
    public void ABodyOnA301RedirectIsStillRefused() =>
        // Belt and braces: a body is never safe to drop, whatever the status.
        Assert.False(TidyDecision.CanTidy(hasRequestBody: true, redirectStatus: 301));
}
