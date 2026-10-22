#pragma once

namespace Integration
{
  bool shouldAsk(bool background);
  bool markAsked();
  bool installMenuEntry();
  bool menuEntryExists();
  /// Removes the menu entry and the icons installMenuEntry() wrote.
  bool removeMenuEntry();
}
