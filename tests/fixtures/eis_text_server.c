/* Private EIS receiver for host_text's ignored integration test.
 * This program does not connect to a desktop or an input device. */
#include <libeis.h>
#include <poll.h>
#include <stdio.h>
#include <time.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    struct eis *context = eis_new(NULL);
    if (!context || eis_setup_backend_socket(context, argv[1]) != 0) return 3;
    struct eis_seat *seat = NULL;
    struct eis_device *device = NULL;
    int keys = 0, frames = 0, disconnected = 0;
    puts("ready");
    fflush(stdout);
    time_t deadline = time(NULL) + 5;
    while (time(NULL) < deadline && !disconnected) {
        struct pollfd descriptor = { .fd = eis_get_fd(context), .events = POLLIN };
        if (poll(&descriptor, 1, 10) < 0) return 4;
        eis_dispatch(context);
        struct eis_event *event;
        while ((event = eis_get_event(context))) {
            switch (eis_event_get_type(event)) {
            case EIS_EVENT_CLIENT_CONNECT:
                eis_client_connect(eis_event_get_client(event));
                seat = eis_client_new_seat(eis_event_get_client(event), "test text seat");
                eis_seat_configure_capability(seat, EIS_DEVICE_CAP_TEXT);
                eis_seat_add(seat);
                break;
            case EIS_EVENT_SEAT_BIND:
                if (!device) {
                    device = eis_seat_new_device(seat);
                    eis_device_configure_type(device, EIS_DEVICE_TYPE_VIRTUAL);
                    eis_device_configure_name(device, "isolated text device");
                    eis_device_configure_capability(device, EIS_DEVICE_CAP_TEXT);
                    eis_device_add(device);
                    eis_device_resume(device);
                }
                break;
            case EIS_EVENT_TEXT_KEYSYM:
                printf("key %u %u\n", eis_event_text_get_keysym(event),
                       eis_event_text_get_keysym_is_press(event));
                keys++;
                break;
            case EIS_EVENT_FRAME:
                frames++;
                if (keys == 10 && frames == 10) {
                    eis_device_remove(device);
                    puts("removed");
                    fflush(stdout);
                }
                break;
            case EIS_EVENT_CLIENT_DISCONNECT:
                disconnected = 1;
                break;
            default:
                break;
            }
            eis_event_unref(event);
        }
    }
    if (device) eis_device_unref(device);
    if (seat) eis_seat_unref(seat);
    eis_unref(context);
    printf("frames %d\ndisconnected %d\n", frames, disconnected);
    return keys == 10 && frames == 10 && disconnected ? 0 : 5;
}
