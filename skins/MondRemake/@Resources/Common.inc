; Shared by every Mond Remake widget: right-click menu, check marks, styles.

[Rainmeter]
Group=MondRemake
AccurateText=1
DynamicWindowSize=1
ContextTitle=#tSettings#
ContextAction=["#@#Settings.inc"]
ContextTitle2=---
ContextAction2=[]
ContextTitle3=#t24h##Mark24#
ContextAction3=[!WriteKeyValue Variables Hour24 1 "#@#Settings.inc"][!RefreshGroup MondRemake]
ContextTitle4=#t12h##Mark12#
ContextAction4=[!WriteKeyValue Variables Hour24 0 "#@#Settings.inc"][!RefreshGroup MondRemake]
ContextTitle5=---
ContextAction5=[]
ContextTitle6=#tCelsius##MarkC#
ContextAction6=[!WriteKeyValue Variables Units celsius "#@#Settings.inc"][!RefreshGroup MondRemake]
ContextTitle7=#tFahrenheit##MarkF#
ContextAction7=[!WriteKeyValue Variables Units fahrenheit "#@#Settings.inc"][!RefreshGroup MondRemake]

[Variables]
Mark24=
Mark12=
MarkC=
MarkF=

; Check marks for the current choices (evaluated once per load).
[MeasureHourMark]
Measure=Calc
Formula=#Hour24#
IfCondition=MeasureHourMark = 1
IfTrueAction=[!SetVariable Mark24 "  [\x2714]"][!SetVariable Mark12 ""]
IfFalseAction=[!SetVariable Mark12 "  [\x2714]"][!SetVariable Mark24 ""]
UpdateDivider=-1

[MeasureUnitsMark]
Measure=String
String=#Units#
IfMatch=(?i)^fahrenheit$
IfMatchAction=[!SetVariable MarkF "  [\x2714]"][!SetVariable MarkC ""]
IfNotMatchAction=[!SetVariable MarkC "  [\x2714]"][!SetVariable MarkF ""]
UpdateDivider=-1

[StyleText]
FontFace=#TextFont#
FontColor=#TextColor#
StringAlign=Center
AntiAlias=1
