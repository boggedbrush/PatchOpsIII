import datetime

def write_log(message, category="Info", log_widget=None, log_file="PatchOpsIII.log"):
    timestamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    full_message = f"{timestamp} - {category}: {message}"
    if log_widget:
        # Define colors based on category
        if category == "Info":
            color = "white"
        elif category == "Error":
            color = "red"
        elif category == "Warning":
            color = "yellow"
        elif category == "Success":
            color = "green"
        else:
            color = "blue"
        html_message = f'<span style="color:{color};">{full_message}</span>'
        log_widget.append(html_message)
    
    with open(log_file, "a") as f:
        f.write(full_message + "\n")

def on_apply_launch_options(self):
    if self.radio_none.isChecked():
        option = ""
    elif self.radio_all_around.isChecked():
        option = "+set fs_game 2994481309"
    elif self.radio_ultimate.isChecked():
        option = "+set fs_game 2942053577"

    try:
        write_log("Applying launch options...", "Info", self.log_widget)
        apply_launch_options(option, self.log_widget)
    except Exception as e:
        write_log(f"Error applying launch options: {e}", "Error", self.log_widget)