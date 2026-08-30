#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int title_matches(Display *display, Window window, const char *wanted) {
  char *title = NULL;
  if (XFetchName(display, window, &title) && title != NULL) {
    int matches = strcmp(title, wanted) == 0;
    XFree(title);
    return matches;
  }
  return 0;
}

static Window find_window(Display *display, Window root, const char *wanted) {
  Window root_return;
  Window parent_return;
  Window *children = NULL;
  unsigned int child_count = 0;

  if (title_matches(display, root, wanted)) {
    return root;
  }
  if (!XQueryTree(display, root, &root_return, &parent_return, &children,
                  &child_count)) {
    return 0;
  }
  for (unsigned int index = 0; index < child_count; index++) {
    Window found = find_window(display, children[index], wanted);
    if (found != 0) {
      XFree(children);
      return found;
    }
  }
  if (children != NULL) {
    XFree(children);
  }
  return 0;
}

int main(int argc, char **argv) {
  const char *title = argc > 1 ? argv[1] : "PatchOpsIII";
  Display *display = XOpenDisplay(NULL);
  if (display == NULL) {
    fputs("could not open X display\n", stderr);
    return 2;
  }

  Window window = 0;
  for (int attempt = 0; attempt < 500 && window == 0; attempt++) {
    window = find_window(display, DefaultRootWindow(display), title);
    if (window == 0) {
      usleep(10000);
    }
  }
  if (window == 0) {
    fprintf(stderr, "window not found: %s\n", title);
    XCloseDisplay(display);
    return 3;
  }

  Atom protocols = XInternAtom(display, "WM_PROTOCOLS", False);
  Atom close = XInternAtom(display, "WM_DELETE_WINDOW", False);
  XEvent event = {0};
  event.xclient.type = ClientMessage;
  event.xclient.window = window;
  event.xclient.message_type = protocols;
  event.xclient.format = 32;
  event.xclient.data.l[0] = (long)close;
  event.xclient.data.l[1] = CurrentTime;
  int sent = XSendEvent(display, window, False, NoEventMask, &event);
  XFlush(display);
  XCloseDisplay(display);
  return sent ? 0 : 4;
}
