def reward_function(params):
    # Extract parameters
    all_wheels_on_track = params['all_wheels_on_track']
    distance_from_center = params['distance_from_center']
    track_width = params['track_width']
    speed = params['speed']
    steering = abs(params['steering_angle'])
    progress = params['progress']
    steps = params['steps']
    
    # Initialize reward - critical for off-track situations
    if not all_wheels_on_track:
        return 1e-3  # Minimal reward if off track
    
    # =====================================
    # RACING LINE COMPONENT
    # =====================================
    # Instead of rewarding center line, use a dynamic acceptable range
    # Wider acceptable range in curves (when steering needed)
    # Narrower range when going straight
    
    # Normalize distance from center
    normalized_distance = distance_from_center / (track_width/2)
    
    # Create adaptive threshold based on steering
    # More steering = wider acceptable range from center
    steering_factor = min(1.0, steering / 15.0)  # Normalize steering up to 15 degrees
    acceptable_distance = 0.3 + (0.5 * steering_factor)  # Range from 0.3 to 0.8
    
    # Racing line reward - higher when following optimal path
    if normalized_distance <= acceptable_distance:
        racing_line_reward = 1.0
    else:
        racing_line_reward = max(0.1, 1.0 - ((normalized_distance - acceptable_distance) * 2))
    
    # =====================================
    # SPEED COMPONENT
    # =====================================
    # Encourage high speed overall
    speed_reward = speed / 4.0  # Normalize assuming max speed of 4 m/s
    
    # =====================================
    # STEERING CONSISTENCY COMPONENT
    # =====================================
    # Reward lower steering angles (more constant steering)
    steering_reward = 1.0 - (steering / 30.0)
    
    # =====================================
    # COMBINE REWARDS
    # =====================================
    # Prioritize racing line (0.4) and speed (0.4), with consistency bonus (0.2)
    reward = (racing_line_reward * 0.4) + (speed_reward * 0.4) + (steering_reward * 0.2)
    
    # Add efficiency bonus
    if steps > 0:
        reward += (progress / steps) * 0.5
    
    return float(reward)
