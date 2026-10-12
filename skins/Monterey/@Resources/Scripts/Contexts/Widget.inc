[Variables]
WidgetContextTitle    = #tEdit# "[#t[#WidgetName]]"
WidgetContextAction   = [!ActivateConfig "#CURRENTCONFIG#" "Settings.ini"]
WidgetContextTitle2   = ---
WidgetContextAction2  = []
WidgetContextTitle3   = #tSmall#
WidgetContextAction3  = [!WriteKeyValue Variables "[#WidgetName]WidgetSize" "Small" "#@#Variables\Layout.inc"][!ActivateConfig "#CURRENTCONFIG#" "Small.ini"]
WidgetContextTitle4   = #tMedium#
WidgetContextAction4  = [!WriteKeyValue Variables "[#WidgetName]WidgetSize" "Medium" "#@#Variables\Layout.inc"][!ActivateConfig "#CURRENTCONFIG#" "Medium.ini"]
WidgetContextTitle5   = #tWide#
WidgetContextAction5  = [!WriteKeyValue Variables "[#WidgetName]WidgetSize" "Wide" "#@#Variables\Layout.inc"][!ActivateConfig "#CURRENTCONFIG#" "Wide.ini"]
WidgetContextTitle6   = #tLarge#
WidgetContextAction6  = [!WriteKeyValue Variables "[#WidgetName]WidgetSize" "Large" "#@#Variables\Layout.inc"][!ActivateConfig "#CURRENTCONFIG#" "Large.ini"]
WidgetContextTitle7   = ---
WidgetContextAction7  = []
WidgetContextTitle8   = #tRemoveWidget#
WidgetContextAction8  = [!DeactivateConfig "#CURRENTCONFIG#"]
WidgetContextTitle9   = ---
WidgetContextAction9  = []
WidgetContextTitle10  = #tSendFeedback# 
WidgetContextAction10 = ["https://forms.gle/ZtCrtG44SFG8eaQKA"]
WidgetContextTitle11  = #tEditWidgets#
WidgetContextAction11 = [!WriteKeyValue Variables SettingsTab "Widgets" "#@#Variables\Layout.inc"][!ToggleConfig "Monterey" "Settings.ini"][!ActivateConfig "Monterey" "Settings.ini"]
WidgetContextAction12 = 

[SetTickMark]
Measure=String
String=#WidgetSize#
IfMatch=Small
IfMatchAction=[!SetVariable WidgetContextTitle3 "#tSmall#  [\x2714]"]
IfMatch2=Medium
IfMatchAction2=[!SetVariable WidgetContextTitle4 "#tMedium#  [\x2714]"]
IfMatch3=Wide
IfMatchAction3=[!SetVariable WidgetContextTitle5 "#tWide#  [\x2714]"]
IfMatch4=Large
IfMatchAction4=[!SetVariable WidgetContextTitle6 "#tLarge#  [\x2714]"]
