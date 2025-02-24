import datetime

def write_log(message, category="Info", log_widget=None, log_file="PatchOpsIII.log"):
    timestamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    full_message = f"{timestamp} - {category}: {message}"
    if log_widget:
        log_widget.append(full_message)
    with open(log_file, "a") as f:
        f.write(full_message + "\n")
